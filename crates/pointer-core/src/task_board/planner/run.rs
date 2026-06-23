//! Planner loop driver.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::AgentProfile;
use crate::chat_service::{emit_task_board_updated, AppState, StreamTx};
use crate::llm_token_stats::{model_name_for_usage_report, ConversationLlmStats};
use crate::models::{ChatMessage, ModelSettings};
use crate::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

use super::history::build_planner_history;
use super::llm::planner_provider;
use super::system::{build_planner_system, PlannerSystemInput};
use super::tool_pass::{
    append_assistant_tool_calls, append_tool_result, dispatch_planner_tool, PlannerToolPassInput,
};
use super::tools::openai_tools;

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
        anchor_message_id: String,
    },
}

pub struct PlannerRunInput<'a> {
    pub state: &'a AppState,
    pub provider: &'a OpenAIProvider,
    pub settings: &'a ModelSettings,
    pub main_history: &'a [ChatMessage],
    pub conversation_id: &'a str,
    pub store_key: &'a str,
    pub lead_agent_id: &'a str,
    pub lead_profile: AgentProfile,
    pub cancel: &'a CancellationToken,
    pub llm_stats: &'a mut ConversationLlmStats,
    pub run_id: &'a str,
    pub stream: &'a StreamTx,
    pub context: PlannerContext,
}

pub async fn run_planner_loop(input: PlannerRunInput<'_>) -> PlannerRunOutcome {
    if input.lead_profile != AgentProfile::Computer {
        return PlannerRunOutcome::NotApplicable;
    }
    if !input.settings.task_board_planner_enabled {
        return PlannerRunOutcome::NotApplicable;
    }

    let store = input.state.task_board_store.clone();
    let doc_before = store.document(input.store_key);
    let planner_history = build_planner_history(input.main_history);
    let today_line = chrono::Local::now()
        .format("[Environment] Today is %A, %Y-%m-%d.")
        .to_string();
    let system = build_planner_system(PlannerSystemInput {
        doc: &doc_before,
        store_key: input.store_key,
        work_items: Some(store.work_items.as_ref()),
        workspace_root: &input.settings.workspace_root,
        today_line: &today_line,
    });
    let planner_provider = planner_provider(input.provider, input.lead_agent_id);
    let native_tools = openai_tools();
    let scope = AgentInstanceScope::new(input.run_id, input.conversation_id, "task_board_planner");
    let work_items_enabled = input.settings.task_board_work_items_enabled;

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

        let mut pass_input = PlannerToolPassInput {
            store: store.clone(),
            store_key: input.store_key,
            conversation_id: input.conversation_id,
            settings: input.settings,
            cancel: input.cancel,
            llm_stats: input.llm_stats,
            run_id: input.run_id,
            work_items_enabled,
            history: input.main_history,
        };

        for tc in &out.tool_calls {
            log::info!(
                "task_board_obs: planner_round={round} tool={}",
                tc.name
            );
            match dispatch_planner_tool(&mut pass_input, tc).await {
                Ok(result) => {
                    if let Some(method) = result.planned {
                        last_planned = Some((method, result.board_len));
                        let anchor = input
                            .state
                            .get_main_task_board_anchor(input.conversation_id, input.store_key);
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
                    append_tool_result(
                        &mut history,
                        &tc.id,
                        &tc.name,
                        &format!("ERROR: {e:#}"),
                    );
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
