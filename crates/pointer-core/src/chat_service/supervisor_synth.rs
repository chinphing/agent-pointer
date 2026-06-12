//! Supervisor final answer synthesis via `chat_once`.

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::agents::{
    expand_agent_prompt_placeholders, rendered_communication_public_inject, AgentRunResult,
    SessionInjectVars,
};
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
    run_id: &str,
    workspace_root: &str,
) -> Result<(String, String)> {
    let mut report = String::new();
    for result in results {
        report.push_str(&format!(
            "## {} ({})\nTask: {}\n{}\n\n",
            result.agent_name, result.agent_id, result.task_id, result.content
        ));
    }
    let env_context = crate::env_prompt::build_environment_context_full();
    let sub_agent_results = if report.is_empty() {
        "No sub-agent results; answer cautiously from the conversation only.".to_string()
    } else {
        report
    };
    let mut prompt_parts: Vec<String> = Vec::new();
    if let Some(block) = rendered_communication_public_inject() {
        let vars = SessionInjectVars {
            workspace_root: workspace_root.trim(),
        };
        prompt_parts.push(expand_agent_prompt_placeholders(&block, &vars));
    }
    prompt_parts.push(env_context);
    prompt_parts.push(
        "You are the Supervisor. From the sub-agent results below, write the final user-facing answer.\n\
Requirements: merge duplicates and resolve conflicts; do not state facts that sub-agents did not support; \
briefly note which agents contributed when helpful."
            .to_string(),
    );
    prompt_parts.push(format!("Sub-agent results:\n{sub_agent_results}"));
    let prompt = prompt_parts.join("\n\n");
    let dump_lbl = format!("{conversation_id}_{assistant_message_id}_supervisor_synthesize");
    let out = provider
        .chat_once(
            history,
            &crate::models::SystemPromptSections::all_cacheable(vec![prompt]),
            Vec::new(),
            cancel,
            None,
            Some(dump_lbl.as_str()),
        )
        .await?;
    let synth_scope =
        crate::agent_instance_scope::AgentInstanceScope::new(run_id, conversation_id, "supervisor");
    let model_name = crate::llm_token_stats::model_name_for_usage_report(&out.model);
    llm_stats.record_llm_round(&synth_scope, out.usage.as_ref(), model_name);
    Ok((out.text, synth_scope.agent_instance_id))
}
