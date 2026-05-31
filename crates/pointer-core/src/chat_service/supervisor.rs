//! Supervisor multi-agent orchestration (`run_supervisor_chat`).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio_util::sync::CancellationToken;

use crate::agents::{AgentRunLimits, AgentRunResult, DEFAULT_AGENT_ID, SUPERVISOR_AGENT_ID};
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::{AgentTrace, ChatMessage, Role, StreamEvent, SupervisorPlanTask};
use crate::provider::OpenAIProvider;

use super::app_state::AppState;
use super::emit::{agent_trace_step_id, emit, emit_agent_step, emit_task_board_updated};
use super::session_budget::SessionToolBudget;
use super::supervisor_plan::{fallback_agent_tasks, plan_agent_tasks, sort_agent_tasks_topologically};
use super::supervisor_synth::synthesize_final_answer;
use super::util::{new_id, now_ms, truncate_str};
use super::StreamTx;
use crate::task_board::{
    check_dependencies, dispatch_to_child, observability, report_child_status,
    sub_agent_task_board_store_key, sync_parent_board_from_supervisor_plan, BoardItem,
    DependencyCheck, ItemStatus,
};

pub(crate) async fn run_supervisor_chat(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &[String],
    provider: OpenAIProvider,
    tool_budget: &mut SessionToolBudget,
    cancel: CancellationToken,
    reasoning_in_messages: bool,
    llm_stats: &mut ConversationLlmStats,
) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(anyhow!("已停止生成"));
    }

    let assistant_id = new_id("msg");
    emit(
        &stream,
        StreamEvent::MessageStart {
            message_id: assistant_id.clone(),
            conversation_id: conversation_id.to_string(),
        },
    );

    let limits = AgentRunLimits::default();
    let mut agent_trace = Vec::new();
    let sup_meta = state.agents.get(SUPERVISOR_AGENT_ID);
    let sup_name = sup_meta
        .as_ref()
        .map(|a| a.def().name.clone())
        .unwrap_or_else(|| "团队模式".into());
    emit_agent_step(
        &stream,
        &assistant_id,
        &mut agent_trace,
        AgentTrace {
            id: "supervisor".into(),
            name: sup_name.clone(),
            role: "supervisor".into(),
            status: "planning".into(),
            detail: Some("正在规划子 Agent 执行任务".into()),
            content: None,
            depth: Some(0),
        },
    );

    let env_context = crate::env_prompt::build_environment_context_full();
    let tasks = match plan_agent_tasks(
        &provider,
        &state,
        history,
        &limits,
        cancel.clone(),
        &env_context,
        conversation_id,
        &assistant_id,
        llm_stats,
    )
    .await
    {
        Ok(tasks) => tasks,
        Err(err) => {
            log::warn!("agent planning failed, fallback to default agent: {err}");
            fallback_agent_tasks(&state, history, &limits)
        }
    };
    let tasks = sort_agent_tasks_topologically(tasks);
    let tasks: Vec<_> = tasks.into_iter().take(limits.max_sub_agents).collect();

    let plan_goal = history
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| truncate_str(&m.content, 240))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Supervisor sub-agent plan".to_string());
    let plan_sync_stats = sync_parent_board_from_supervisor_plan(
        &state.task_board_store,
        conversation_id,
        &tasks,
        &plan_goal,
    );
    observability::log_supervisor_plan_sync(conversation_id, &plan_sync_stats, tasks.len());

    let plan_tasks: Vec<SupervisorPlanTask> = tasks
        .iter()
        .map(|t| SupervisorPlanTask {
            id: t.id.clone(),
            title: if t.title.trim().is_empty() {
                truncate_str(&t.instruction, 120)
            } else {
                t.title.trim().to_string()
            },
            agent_id: t.agent_id.clone(),
        })
        .collect();
    emit(
        &stream,
        StreamEvent::SupervisorPlan {
            conversation_id: conversation_id.to_string(),
            message_id: assistant_id.clone(),
            tasks: plan_tasks,
        },
    );
    let parent_doc = state.task_board_store.document(conversation_id);
    emit_task_board_updated(
        &stream,
        conversation_id,
        conversation_id,
        None,
        parent_doc.to_value(),
    );

    let mut results = Vec::new();
    let mut results_by_id: HashMap<String, AgentRunResult> = HashMap::new();
    for task in tasks {
        if cancel.is_cancelled() {
            return Err(anyhow!("已停止生成"));
        }
        if tool_budget.remaining() == 0 {
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!(
                "本会话在编排模式下可执行的子任务次数已达上限（{}），请新开对话或在设置中调高上限。",
                tool_budget.cap()
            ));
        }
        let agent = state
            .agents
            .get(&task.agent_id)
            .or_else(|| state.agents.get(DEFAULT_AGENT_ID))
            .ok_or_else(|| anyhow!("未找到可执行 Agent: {}", task.agent_id))?;
        let def = agent.def();
        emit_agent_step(
            &stream,
            &assistant_id,
            &mut agent_trace,
            AgentTrace {
                id: agent_trace_step_id(&task.id, &def.id),
                name: def.name.clone(),
                role: def.role.clone(),
                status: "running".into(),
                detail: Some(if task.title.is_empty() {
                    task.instruction.clone()
                } else {
                    task.title.clone()
                }),
                content: Some(String::new()),
                depth: Some(1),
            },
        );

        let parent_board_key = conversation_id;
        let child_board_key = sub_agent_task_board_store_key(conversation_id, task.id.trim());
        let parent_doc = state.task_board_store.document(parent_board_key);
        if let DependencyCheck::Blocked { reason } = check_dependencies(&parent_doc, &task.id) {
            log::warn!(
                "supervisor: task_board dependency blocked task_id={} reason={reason}",
                task.id
            );
        }
        let milestone = BoardItem {
            id: task.id.clone(),
            title: if task.title.trim().is_empty() {
                format!("Sub-task {}", task.id.trim())
            } else {
                task.title.trim().to_string()
            },
            status: ItemStatus::Pending,
            depends_on: task.depends_on.clone(),
            validate: Some(truncate_str(&task.instruction, 160)),
            ..BoardItem::default()
        };
        let child_was_empty = state
            .task_board_store
            .document(&child_board_key)
            .board
            .is_empty();
        if let Err(err) = dispatch_to_child(
            &state.task_board_store,
            parent_board_key,
            &child_board_key,
            task.id.trim(),
            Some(milestone),
        ) {
            log::warn!("supervisor: dispatch_to_child failed: {err}");
        } else {
            let child_seeded = child_was_empty
                && !state
                    .task_board_store
                    .document(&child_board_key)
                    .board
                    .is_empty();
            observability::log_dispatch_child(conversation_id, task.id.trim(), child_seeded);
            let child_doc = state.task_board_store.document(&child_board_key);
            emit_task_board_updated(
                &stream,
                conversation_id,
                &child_board_key,
                None,
                child_doc.to_value(),
            );
        }

        let mut task_run = task.clone();
        if !task.depends_on.is_empty() {
            let mut pre = String::from("\n\n[Prior task outputs]\n");
            for d in &task.depends_on {
                if let Some(r) = results_by_id.get(d) {
                    pre.push_str(&format!(
                        "--- task {} ---\n{}\n",
                        d,
                        truncate_str(&r.content, 2000)
                    ));
                }
            }
            task_run.instruction = format!("{pre}\n\n[Current task]\n{}", task.instruction);
        }

        let sub_cap = provider.settings.max_sub_agent_tool_rounds.clamp(1, 10_000);
        let mut sub_budget = SessionToolBudget::new(sub_cap, 0);
        match super::sub_agent::run_sub_agent(
            &provider,
            &state,
            &stream,
            conversation_id,
            &assistant_id,
            &mut agent_trace,
            enabled_skill_ids,
            &task_run,
            &mut sub_budget,
            cancel.clone(),
            reasoning_in_messages,
            llm_stats,
        )
        .await
        {
            Ok(result) => {
                tool_budget.record_tool_cycle();
                let mut parent = state.task_board_store.document(parent_board_key);
                if parent.board.iter().any(|i| i.id == task.id) {
                    if let Err(err) = report_child_status(
                        &mut parent,
                        task.id.trim(),
                        ItemStatus::Done,
                        &result.content,
                    ) {
                        log::warn!("supervisor: report_child_status failed: {err}");
                    } else {
                        state.task_board_store.save_document(parent_board_key, parent.clone());
                        emit_task_board_updated(
                            &stream,
                            conversation_id,
                            parent_board_key,
                            None,
                            parent.to_value(),
                        );
                    }
                }
                results_by_id.insert(task.id.clone(), result.clone());
                emit_agent_step(
                    &stream,
                    &assistant_id,
                    &mut agent_trace,
                    AgentTrace {
                        id: agent_trace_step_id(&task.id, &def.id),
                        name: def.name.clone(),
                        role: def.role.clone(),
                        status: "completed".into(),
                        detail: Some(truncate_str(&result.content, 160)),
                        content: Some(result.content.clone()),
                        depth: Some(1),
                    },
                );
                results.push(result);
            }
            Err(err) => {
                let mut parent = state.task_board_store.document(parent_board_key);
                if parent.board.iter().any(|i| i.id == task.id) {
                    let note = err.to_string();
                    if let Err(rep) = report_child_status(
                        &mut parent,
                        task.id.trim(),
                        ItemStatus::Failed,
                        &note,
                    ) {
                        log::warn!("supervisor: report_child_status (failed) err: {rep}");
                    } else {
                        state.task_board_store.save_document(parent_board_key, parent.clone());
                        emit_task_board_updated(
                            &stream,
                            conversation_id,
                            parent_board_key,
                            None,
                            parent.to_value(),
                        );
                    }
                }
                emit_agent_step(
                    &stream,
                    &assistant_id,
                    &mut agent_trace,
                    AgentTrace {
                        id: agent_trace_step_id(&task.id, &def.id),
                        name: def.name.clone(),
                        role: def.role.clone(),
                        status: "failed".into(),
                        detail: Some(err.to_string()),
                        content: None,
                        depth: Some(1),
                    },
                );
            }
        }
    }

    emit_agent_step(
        &stream,
        &assistant_id,
        &mut agent_trace,
        AgentTrace {
            id: "supervisor".into(),
            name: sup_name.clone(),
            role: "supervisor".into(),
            status: "summarizing".into(),
            detail: Some("正在整合子 Agent 结果".into()),
            content: None,
            depth: Some(0),
        },
    );

    let (final_answer, synth_instance_id) = synthesize_final_answer(
        &provider,
        history,
        &results,
        cancel,
        conversation_id,
        &assistant_id,
        llm_stats,
    )
    .await?;
    if !final_answer.is_empty() {
        emit(
            &stream,
            StreamEvent::Delta {
                message_id: assistant_id.clone(),
                text: final_answer.clone(),
            },
        );
    }

    history.push(ChatMessage {
        id: assistant_id.clone(),
        role: Role::Assistant,
        content: final_answer.clone(),
        status: "completed".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        tool_raw_output: None,
        agent_id: Some("supervisor".into()),
        agent_instance_id: Some(synth_instance_id),
        agent_name: Some(sup_name),
        agent_trace: Some(agent_trace),
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
            context_state: None,
            });
    emit(
        &stream,
        StreamEvent::MessageEnd {
            message_id: assistant_id,
            content: Some(final_answer),
            raw_content: None,
            tool_raw_output: None,
            thoughts: None,
            headline: None,
            trace_id: None,
        },
    );
    state.computer_state.mark_ended(conversation_id);
    Ok(())
}
