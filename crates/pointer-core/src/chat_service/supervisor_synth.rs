//! Supervisor final answer synthesis via `chat_once`.

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::agents::AgentRunResult;
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::ChatMessage;
use crate::provider::OpenAIProvider;

pub(crate) async fn synthesize_final_answer(
    provider: &OpenAIProvider,
    history: &[ChatMessage],
    results: &[AgentRunResult],
    cancel: CancellationToken,
    conversation_id: &str,
    assistant_message_id: &str,
    llm_stats: &mut ConversationLlmStats,
) -> Result<(String, String)> {
    let mut report = String::new();
    for result in results {
        report.push_str(&format!(
            "## {} ({})\nTask: {}\n{}\n\n",
            result.agent_name, result.agent_id, result.task_id, result.content
        ));
    }
    let env_context = crate::env_prompt::build_environment_context_full();
    let prompt = format!(
        "{}\n\nYou are the Supervisor. From the sub-agent results below, write the final user-facing answer.\nRequirements: merge duplicates and resolve conflicts; do not state facts that sub-agents did not support; briefly note which agents contributed when helpful.\n\nSub-agent results:\n{}",
        env_context,
        if report.is_empty() {
            "No sub-agent results; answer cautiously from the conversation only.".into()
        } else {
            report
        }
    );
    let dump_lbl = format!("{conversation_id}_{assistant_message_id}_supervisor_synthesize");
    let out = provider
        .chat_once(
            history,
            &crate::models::SystemPromptSections::all_cacheable(vec![prompt]),
            cancel,
            None,
            Some(dump_lbl.as_str()),
        )
        .await?;
    let synth_scope =
        crate::agent_instance_scope::AgentInstanceScope::new(conversation_id, "supervisor");
    let model_name = if provider.settings.model.trim().is_empty() {
        None
    } else {
        Some(provider.settings.model.as_str())
    };
    llm_stats.record_llm_round(&synth_scope, out.usage.as_ref(), model_name);
    Ok((out.text, synth_scope.agent_instance_id))
}
