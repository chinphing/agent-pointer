//! Planner loop driver.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::AgentProfile;
use crate::chat_service::{emit_task_board_updated, AppState, StreamTx};
use crate::task_board::anchor_message_id_from_main_turn_key;
use crate::llm_token_stats::{model_name_for_usage_report, ConversationLlmStats};
use crate::models::{ChatMessage, ModelSettings};
use crate::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

use super::history::build_planner_history;
use super::llm::planner_provider;
use super::system::{build_planner_system, PlannerSystemInput};
use super::tool_pass::{
    append_assistant_tool_calls, append_tool_result, dispatch_planner_tool,
    PlannerToolOutcome, PlannerToolPassInput,
};
use super::stream_ui::PlannerUiTarget;
use super::tools::openai_tools;
use std::time::Instant;

pub const PLANNER_MAX_TOOL_ROUNDS: u32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannerRunOutcome {
    NotApplicable,
    Skipped { reason: String },
    Planned {
        method: PlannedMethod,
        board_len: usize,
    },
    Failed { error: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedMethod {
    Init,
    Replace,
}

#[derive(Debug, Clone)]
pub enum PlannerContext {
    MainTurn,
    SubAgent {
        /// Lead assistant message id (legacy UI binding fallback).
        anchor_message_id: String,
        /// Sub-agent trace id (`{taskId}:{agentId}`) for TaskBoardPanel binding.
        trace_id: String,
    },
}

/// Stream anchor for `task_board_updated` — matches execution tool-pass binding.
pub fn planner_task_board_emit_anchor(
    context: &PlannerContext,
    state: &AppState,
    conversation_id: &str,
    store_key: &str,
) -> Option<String> {
    match context {
        PlannerContext::SubAgent { trace_id, anchor_message_id } => {
            let trace = trace_id.trim();
            if !trace.is_empty() {
                Some(trace.to_string())
            } else {
                let lead = anchor_message_id.trim();
                if lead.is_empty() {
                    None
                } else {
                    Some(lead.to_string())
                }
            }
        }
        PlannerContext::MainTurn => state
            .get_main_task_board_anchor(conversation_id, store_key)
            .or_else(|| anchor_message_id_from_main_turn_key(store_key)),
    }
}

pub struct PlannerRunInput<'a> {
    pub state: &'a AppState,
    pub provider: &'a OpenAIProvider,
    pub settings: &'a ModelSettings,
    pub main_history: &'a mut Vec<ChatMessage>,
    pub conversation_id: &'a str,
    pub store_key: &'a str,
    pub lead_agent_id: &'a str,
    pub lead_profile: AgentProfile,
    pub cancel: &'a CancellationToken,
    pub llm_stats: &'a mut ConversationLlmStats,
    pub run_id: &'a str,
    pub stream: &'a StreamTx,
    pub context: PlannerContext,
    /// Sub-agent delegated task system dynamic (empty for main-turn planner).
    pub system_dynamic: &'a [String],
    /// When set, planner tool rounds are mirrored to the chat UI.
    pub ui: Option<PlannerUiTarget<'a>>,
}

pub async fn run_planner_loop(mut input: PlannerRunInput<'_>) -> PlannerRunOutcome {
    let scoped_ui = input
        .ui
        .as_ref()
        .map(|u| u.scoped_message_id.is_some())
        .unwrap_or(false);
    let outcome = run_planner_loop_body(&mut input).await;
    if let Some(ui) = input.ui.as_ref() {
        let history = if scoped_ui {
            None
        } else {
            Some(&mut *input.main_history)
        };
        ui.emit_phase_complete(history);
    }
    outcome
}

async fn run_planner_loop_body(input: &mut PlannerRunInput<'_>) -> PlannerRunOutcome {
    if input.lead_profile != AgentProfile::Computer {
        return PlannerRunOutcome::NotApplicable;
    }
    if !input.settings.computer_standalone_planner_enabled {
        return PlannerRunOutcome::NotApplicable;
    }

    let store = input.state.task_board_store.clone();
    let doc_before = store.document(input.store_key);
    let planner_history = build_planner_history(&*input.main_history);
    let today_line = chrono::Local::now()
        .format("[Environment] Today is %A, %Y-%m-%d.")
        .to_string();
    let system = build_planner_system(PlannerSystemInput {
        doc: &doc_before,
        store_key: input.store_key,
        work_items: Some(store.work_items.as_ref()),
        workspace_root: &input.settings.workspace_root,
        today_line: &today_line,
        system_dynamic: input.system_dynamic,
    });
    let planner_provider = planner_provider(input.provider, input.lead_agent_id);
    let native_tools = openai_tools();
    let scope = AgentInstanceScope::new(input.run_id, input.conversation_id, "task_board_planner");
    let work_items_enabled = input.settings.computer_standalone_planner_enabled;

    if let Some(ui) = input.ui.as_ref() {
        ui.emit_phase_start();
    }

    let mut history = planner_history;
    let mut round = 0u32;
    let mut last_planned: Option<(PlannedMethod, usize)> = None;

    while round < PLANNER_MAX_TOOL_ROUNDS {
        if input.cancel.is_cancelled() {
            return PlannerRunOutcome::Failed {
                error: "cancelled".into(),
            };
        }
        round += 1;
        let dump_label = format!(
            "{}_{}_task_board_planner_r{round}",
            input.conversation_id,
            input.store_key
        );
        let out = match planner_provider
            .chat_once(
                &history,
                &system,
                native_tools.clone(),
                input.cancel.clone(),
                None,
                Some(dump_label.as_str()),
            )
            .await
        {
            Ok(o) => o,
            Err(e) => {
                log::warn!(
                    "task_board_obs: planner_round={round} outcome=failed error={e:#}"
                );
                return PlannerRunOutcome::Failed {
                    error: format!("{e:#}"),
                };
            }
        };
        let model_name = model_name_for_usage_report(&out.model);
        if let Some(u) = out.usage.as_ref() {
            if u.total_tokens == 0 && u.prompt_tokens == 0 && u.completion_tokens == 0 {
                log::warn!(
                    "task_board_obs: planner_round={round} usage_all_zero model={}",
                    out.model
                );
            }
        } else {
            log::warn!(
                "task_board_obs: planner_round={round} usage_missing model={}",
                out.model
            );
        }
        input
            .llm_stats
            .record_llm_round(&scope, out.usage.as_ref(), model_name);

        if out.tool_calls.is_empty() {
            log::info!(
                "task_board_obs: planner_round={round} outcome=no_tools planned={:?}",
                last_planned.as_ref().map(|(m, _)| m)
            );
            break;
        }

        append_assistant_tool_calls(&mut history, &out.text, &out.tool_calls);

        for tc in &out.tool_calls {
            log::info!(
                "task_board_obs: planner_round={round} tool={}",
                tc.name
            );
            let args = crate::tools::parse_tool_call_arguments(&tc.arguments);
            let tool_start = Instant::now();
            let scoped_ui = input
                .ui
                .as_ref()
                .map(|u| u.scoped_message_id.is_some())
                .unwrap_or(false);
            if let Some(ui) = input.ui.as_ref() {
                let history_patch = if scoped_ui {
                    None
                } else {
                    Some(&mut *input.main_history)
                };
                ui.emit_tool_start(tc, history_patch);
            }
            let mut pass_input = PlannerToolPassInput {
                store: store.clone(),
                store_key: input.store_key,
                settings: input.settings,
                work_items_enabled,
            };
            let dispatch_result = dispatch_planner_tool(&mut pass_input, tc).await;
            let duration_ms = tool_start.elapsed().as_millis() as u64;
            match dispatch_result {
                Ok(result) => {
                    if let Some(ui) = input.ui.as_ref() {
                        let history_patch = if ui.scoped_message_id.is_some() {
                            None
                        } else {
                            Some(&mut *input.main_history)
                        };
                        ui.emit_tool_complete(
                            tc,
                            &args,
                            &result,
                            duration_ms,
                            true,
                            history_patch,
                        );
                    }
                    if let Some(method) = result.planned {
                        last_planned = Some((method, result.board_len));
                        let anchor = planner_task_board_emit_anchor(
                            &input.context,
                            input.state,
                            input.conversation_id,
                            input.store_key,
                        );
                        emit_task_board_updated(
                            input.stream,
                            input.conversation_id,
                            input.store_key,
                            anchor,
                            store.items_json(input.store_key),
                        );
                    }
                    append_tool_result(&mut history, &tc.id, &tc.name, &result.tool_result);
                }
                Err(e) => {
                    log::warn!("task_board_obs: planner tool {} failed: {e:#}", tc.name);
                    let err = format!("ERROR: {e:#}");
                    if let Some(ui) = input.ui.as_ref() {
                        let fail_outcome = PlannerToolOutcome {
                            tool_result: err.clone(),
                            planned: None,
                            board_len: 0,
                        };
                        let history_patch = if ui.scoped_message_id.is_some() {
                            None
                        } else {
                            Some(&mut *input.main_history)
                        };
                        ui.emit_tool_complete(
                            tc,
                            &args,
                            &fail_outcome,
                            duration_ms,
                            false,
                            history_patch,
                        );
                    }
                    append_tool_result(&mut history, &tc.id, &tc.name, &err);
                }
            }
        }
    }

    match last_planned {
        Some((method, board_len)) => {
            log::info!(
                "task_board_obs: planner outcome=planned method={method:?} board_len={board_len}"
            );
            PlannerRunOutcome::Planned {
                method,
                board_len,
            }
        }
        None => {
            let reason = if round >= PLANNER_MAX_TOOL_ROUNDS {
                "max_rounds_without_plan".into()
            } else {
                "no_init_or_replace".into()
            };
            log::info!("task_board_obs: planner outcome=skipped reason={reason}");
            PlannerRunOutcome::Skipped { reason }
        }
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;

    #[test]
    fn sub_agent_emit_anchor_prefers_trace_id() {
        let ctx = PlannerContext::SubAgent {
            anchor_message_id: "lead-msg".into(),
            trace_id: "task_a:computer".into(),
        };
        let anchor = match &ctx {
            PlannerContext::SubAgent { trace_id, .. } => Some(trace_id.clone()),
            PlannerContext::MainTurn => None,
        };
        assert_eq!(anchor.as_deref(), Some("task_a:computer"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentProfile;
    use crate::chat_service::AppState;
    use crate::llm_token_stats::ConversationLlmStats;
    use crate::models::{ChatMessage, ModelSettings, ProviderConfig, Role};
    use crate::provider::OpenAIProvider;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            id: "u1".into(),
            role: Role::User,
            content: content.into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        }
    }

    fn planner_settings(base_url: &str) -> ModelSettings {
        ModelSettings {
            providers: vec![ProviderConfig {
                id: "qwen".into(),
                name: "Qwen".into(),
                base_url: base_url.into(),
                api_key: "test-key".into(),
                models: vec!["qwen3.5-plus".into()],
                reasoning_in_messages: None,
                temperature: None,
                max_tokens: None,
                model_configs: Default::default(),
                enable_thinking: None,
                thinking_budget: None,
                reasoning_effort: None,
            }],
            active_provider_id: "qwen".into(),
            model: "qwen3.5-plus".into(),
            api_key: "test-key".into(),
            computer_standalone_planner_enabled: true,
            ..Default::default()
        }
    }

    async fn mount_chat_completions_mocks(server: &MockServer, init_then_done: bool) {
        let done = ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "content": "planning complete" }
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        }));
        if !init_then_done {
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(done)
                .mount(server)
                .await;
            return;
        }
        let init = ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_init",
                        "type": "function",
                        "function": {
                            "name": "task_board_init",
                            "arguments": "{\"goal\":\"planner goal\",\"items\":[{\"id\":\"s1\",\"title\":\"Step\",\"status\":\"pending\"}]}"
                        }
                    }]
                }
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        }));
        let call_count = Arc::new(Mutex::new(0usize));
        let counter = call_count.clone();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(move |_req: &wiremock::Request| {
                let mut n = counter.lock().expect("mock counter lock");
                *n += 1;
                if *n == 1 {
                    init.clone()
                } else {
                    done.clone()
                }
            })
            .mount(server)
            .await;
    }

    async fn run_with_mock(
        settings: ModelSettings,
        history: &mut Vec<ChatMessage>,
        cancel: CancellationToken,
    ) -> PlannerRunOutcome {
        let state = Arc::new(AppState::new());
        let provider = OpenAIProvider::new(settings.clone(), settings.api_key.clone());
        let (stream, _rx) = mpsc::unbounded_channel();
        let mut llm_stats = ConversationLlmStats::default();
        let store_key = "conv-planner-loop";
        run_planner_loop(PlannerRunInput {
            state: state.as_ref(),
            provider: &provider,
            settings: &settings,
            main_history: history,
            conversation_id: store_key,
            store_key,
            lead_agent_id: "computer",
            lead_profile: AgentProfile::Computer,
            cancel: &cancel,
            llm_stats: &mut llm_stats,
            run_id: "run_planner_test",
            stream: &stream,
            context: PlannerContext::MainTurn,
            system_dynamic: &[],
            ui: None,
        })
        .await
    }

    #[tokio::test]
    async fn not_applicable_for_non_computer_profile() {
        let settings = planner_settings("http://unused");
        let cancel = CancellationToken::new();
        let state = Arc::new(AppState::new());
        let provider = OpenAIProvider::new(settings.clone(), settings.api_key.clone());
        let (stream, _rx) = mpsc::unbounded_channel();
        let mut llm_stats = ConversationLlmStats::default();
        let mut history = vec![user_msg("hello")];
        let outcome = run_planner_loop(PlannerRunInput {
            state: state.as_ref(),
            provider: &provider,
            settings: &settings,
            main_history: &mut history,
            conversation_id: "c1",
            store_key: "c1",
            lead_agent_id: "general",
            lead_profile: AgentProfile::General,
            cancel: &cancel,
            llm_stats: &mut llm_stats,
            run_id: "run",
            stream: &stream,
            context: PlannerContext::MainTurn,
            system_dynamic: &[],
            ui: None,
        })
        .await;
        assert_eq!(outcome, PlannerRunOutcome::NotApplicable);
    }

    #[tokio::test]
    async fn not_applicable_when_planner_disabled() {
        let mut settings = planner_settings("http://unused");
        settings.computer_standalone_planner_enabled = false;
        let mut history = vec![user_msg("hello")];
        let outcome = run_with_mock(settings, &mut history, CancellationToken::new()).await;
        assert_eq!(outcome, PlannerRunOutcome::NotApplicable);
    }

    #[tokio::test]
    async fn failed_when_cancelled_before_llm() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut history = vec![user_msg("hello")];
        let outcome = run_with_mock(
            planner_settings("http://unused"),
            &mut history,
            cancel,
        )
        .await;
        assert_eq!(
            outcome,
            PlannerRunOutcome::Failed {
                error: "cancelled".into()
            }
        );
    }

    #[tokio::test]
    async fn skipped_when_llm_returns_no_planning_tools() {
        let server = MockServer::start().await;
        mount_chat_completions_mocks(&server, false).await;
        let mut history = vec![user_msg("plan something")];
        let outcome = run_with_mock(
            planner_settings(&server.uri()),
            &mut history,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(
            outcome,
            PlannerRunOutcome::Skipped {
                reason: "no_init_or_replace".into()
            }
        );
    }

    #[tokio::test]
    async fn planned_when_init_tool_succeeds() {
        let server = MockServer::start().await;
        mount_chat_completions_mocks(&server, true).await;
        let settings = planner_settings(&server.uri());
        let store_key = format!("conv-planned-{}", uuid::Uuid::new_v4());
        let state = Arc::new(AppState::new());
        let provider = OpenAIProvider::new(settings.clone(), settings.api_key.clone());
        let (stream, _rx) = mpsc::unbounded_channel();
        let mut llm_stats = ConversationLlmStats::default();
        let mut history = vec![user_msg("open ten apps")];
        let outcome = run_planner_loop(PlannerRunInput {
            state: state.as_ref(),
            provider: &provider,
            settings: &settings,
            main_history: &mut history,
            conversation_id: &store_key,
            store_key: &store_key,
            lead_agent_id: "computer",
            lead_profile: AgentProfile::Computer,
            cancel: &CancellationToken::new(),
            llm_stats: &mut llm_stats,
            run_id: "run_planned",
            stream: &stream,
            context: PlannerContext::MainTurn,
            system_dynamic: &[],
            ui: None,
        })
        .await;
        assert_eq!(
            outcome,
            PlannerRunOutcome::Planned {
                method: PlannedMethod::Init,
                board_len: 1,
            }
        );
        assert_eq!(
            state.task_board_store.document(&store_key).meta.goal,
            "planner goal"
        );
    }
}
