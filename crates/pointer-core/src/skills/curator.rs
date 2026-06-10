//! Skill library curator: auto maintenance + periodic LLM review of ~/.pointer/skills.

use super::external::{pointer_home_dir, pointer_skills_dir};
use crate::chat_service::AppState;
use crate::memory::{allowed_tools_for, dispatch_review_tool, ReviewKind};
use crate::models::{ChatMessage, ModelSettings, Role, SystemPromptSections};
use crate::provider::OpenAIProvider;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

const CURATOR_PROMPT: &str = include_str!("prompts/curator.md");
const CURATOR_META_FILE: &str = "curator_meta.json";
const STALE_DAYS: u64 = 30;
const ARCHIVE_DAYS: u64 = 90;
const DAY_MS: i64 = 86_400_000;
const REVIEW_MAX_ITERATIONS: u32 = 12;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CuratorMeta {
    #[serde(default, rename = "lastLlmRunMs")]
    last_llm_run_ms: i64,
    #[serde(default, rename = "lastMaintenanceMs")]
    last_maintenance_ms: i64,
    #[serde(default, rename = "skillUsageMs")]
    skill_usage_ms: std::collections::HashMap<String, i64>,
}

#[derive(Debug, Default)]
pub struct MaintenanceReport {
    pub stale_marked: Vec<String>,
    pub archived: Vec<String>,
}

pub fn record_skill_usage(skill_id: &str) {
    let Ok(mut meta) = load_meta() else {
        return;
    };
    meta.skill_usage_ms
        .insert(skill_id.trim().to_string(), now_ms());
    if let Err(e) = save_meta(&meta) {
        log::warn!("curator: record_skill_usage save failed: {e:#}");
    }
}

pub fn run_auto_maintenance() -> Result<MaintenanceReport> {
    let root = pointer_skills_dir()?;
    let archive_root = root.join(".archive");
    fs::create_dir_all(&archive_root)?;
    let mut meta = load_meta()?;
    let now = now_ms();
    let mut report = MaintenanceReport::default();

    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if !is_managed_skill_dir(&path) {
            continue;
        }
        let last_used = meta
            .skill_usage_ms
            .get(name)
            .copied()
            .unwrap_or_else(|| dir_mtime_ms(&path).unwrap_or(now));
        let age_days = ((now - last_used).max(0) as u64) / (DAY_MS as u64);
        if age_days >= ARCHIVE_DAYS {
            let dest = archive_root.join(name);
            if dest.exists() {
                fs::remove_dir_all(&dest)?;
            }
            fs::rename(&path, &dest)?;
            meta.skill_usage_ms.remove(name);
            report.archived.push(name.to_string());
            log::info!("curator: archived skill {name} (unused {age_days}d)");
        } else if age_days >= STALE_DAYS {
            let marker = path.join(".pointer-stale");
            if !marker.exists() {
                fs::write(&marker, format!("marked_at_ms={now}\n"))?;
                report.stale_marked.push(name.to_string());
                log::info!("curator: marked stale skill {name} (unused {age_days}d)");
            }
        }
    }

    meta.last_maintenance_ms = now;
    save_meta(&meta)?;
    if !report.stale_marked.is_empty() || !report.archived.is_empty() {
        log::info!(
            "curator: maintenance stale={} archived={}",
            report.stale_marked.len(),
            report.archived.len()
        );
    }
    Ok(report)
}

pub fn is_idle_long_enough(state: &AppState, settings: &ModelSettings) -> bool {
    let idle_hours = settings.curator_idle_hours.max(1) as u64;
    let elapsed = state.idle_duration();
    elapsed >= Duration::from_secs(idle_hours * 3600)
}

pub fn llm_curator_due(settings: &ModelSettings) -> bool {
    if settings.curator_interval_days == 0 {
        return false;
    }
    let meta = load_meta().unwrap_or_default();
    if meta.last_llm_run_ms <= 0 {
        return true;
    }
    let interval_ms = i64::from(settings.curator_interval_days.max(1)) * DAY_MS;
    now_ms().saturating_sub(meta.last_llm_run_ms) >= interval_ms
}

pub async fn run_llm_curator(
    state: Arc<AppState>,
    provider: OpenAIProvider,
    settings: &ModelSettings,
) -> Result<()> {
    let skills = state.skills.clone();
    let _ = skills.reload_meta();
    let catalog = build_skill_catalog(&skills);
    let tools = state.tools.clone();
    let allowed = allowed_tools_for(ReviewKind::SkillOnly);
    let native_tools = tools.openai_tools(&allowed);
    if native_tools.is_empty() {
        return Err(anyhow!("curator: skill tools not registered"));
    }

    let mut messages = vec![curator_user_message(&catalog)];
    let system = SystemPromptSections::all_cacheable(vec![]);
    let cancel = CancellationToken::new();
    let mut patches = 0u32;

    for iter in 0..REVIEW_MAX_ITERATIONS {
        let out = provider
            .chat_once(
                &messages,
                &system,
                native_tools.clone(),
                cancel.clone(),
                Some(settings.context_summary_max_tokens.min(2048)),
                Some(&format!("skill_curator_{iter}")),
            )
            .await?;

        if out.tool_calls.is_empty() {
            break;
        }

        messages.push(ChatMessage {
            id: format!("curator_a_{iter}"),
            role: Role::Assistant,
            content: out.text.clone(),
            status: "done".into(),
            created_at: now_ms(),
            tool_calls: Some(out.tool_calls.clone()),
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
        });

        for tc in &out.tool_calls {
            let result = dispatch_review_tool(
                &state.memory_store,
                &skills,
                settings,
                &tc.name,
                &tc.arguments,
            )
            .unwrap_or_else(|e: anyhow::Error| {
                serde_json::json!({ "success": false, "error": e.to_string() }).to_string()
            });
            if tc.name == "skill_patch_instructions" {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
                    if v.get("success").and_then(|b| b.as_bool()) == Some(true) {
                        patches += 1;
                    }
                }
            }
            messages.push(ChatMessage {
                id: format!("curator_t_{iter}_{}", tc.id),
                role: Role::Tool,
                content: result,
                status: "done".into(),
                created_at: now_ms(),
                tool_calls: None,
                tool_call_id: Some(tc.id.clone()),
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
            });
        }
    }

    let mut meta = load_meta()?;
    meta.last_llm_run_ms = now_ms();
    save_meta(&meta)?;
    log::info!("curator: llm pass finished patches={patches}");
    Ok(())
}

fn build_skill_catalog(skills: &super::SkillRegistry) -> String {
    let mut lines = vec![
        "Current skill library under ~/.pointer/skills:".to_string(),
        String::new(),
    ];
    for s in skills.list() {
        if s.builtin || !s.mutable {
            continue;
        }
        if !super::provenance::is_curation_eligible(&s.id) {
            continue;
        }
        lines.push(format!("- id: {}", s.id));
        lines.push(format!("  name: {}", s.name));
        lines.push(format!("  description: {}", s.description));
        if !s.tags.is_empty() {
            lines.push(format!("  tags: {}", s.tags.join(", ")));
        }
    }
    lines.join("\n")
}

fn curator_user_message(catalog: &str) -> ChatMessage {
    let content = format!("{CURATOR_PROMPT}\n\n{catalog}");
    ChatMessage {
        id: format!("curator_{}", uuid::Uuid::new_v4().simple()),
        role: Role::User,
        content,
        status: "done".into(),
        created_at: now_ms(),
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
    }
}

fn is_managed_skill_dir(dir: &Path) -> bool {
    super::external::is_skill_package_dir(dir)
}

fn meta_path() -> Result<PathBuf> {
    Ok(pointer_home_dir()?.join(CURATOR_META_FILE))
}

fn load_meta() -> Result<CuratorMeta> {
    let path = meta_path()?;
    if !path.exists() {
        return Ok(CuratorMeta::default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_meta(meta: &CuratorMeta) -> Result<()> {
    let path = meta_path()?;
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(meta)?;
    fs::write(&tmp, raw)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn dir_mtime_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct CuratorRunGuard<'a>(&'a AtomicBool);

impl Drop for CuratorRunGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) async fn try_background_provider(state: &AppState) -> Option<OpenAIProvider> {
    if !state.platform_auth.session_view().logged_in {
        return None;
    }
    if state.platform_auth.ensure_llm_allowed().await.is_err() {
        return None;
    }
    let mut settings = state.effective_settings();
    settings.agent_mode = "single".into();
    settings.lead_agent_id = "general".into();
    let api_key =
        crate::chat_service::prepare_session_llm_settings(&mut settings, "single");
    if api_key.is_empty() {
        return None;
    }
    Some(OpenAIProvider::new(settings, api_key))
}

pub fn start_background_loop(state: Arc<AppState>) {
    tokio::spawn(async move {
        if let Err(e) = run_auto_maintenance() {
            log::warn!("curator: startup maintenance failed: {e:#}");
        }
        let mut interval = tokio::time::interval(Duration::from_secs(1800));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(e) = run_auto_maintenance() {
                log::warn!("curator: periodic maintenance failed: {e:#}");
            }
            let settings = state.effective_settings();
            if !settings.curator_enabled {
                continue;
            }
            if !is_idle_long_enough(state.as_ref(), &settings) {
                continue;
            }
            if !llm_curator_due(&settings) {
                continue;
            }
            if state
                .curator_llm_running
                .swap(true, Ordering::Acquire)
            {
                continue;
            }
            let st = state.clone();
            tokio::spawn(async move {
                let _guard = CuratorRunGuard(&st.curator_llm_running);
                match try_background_provider(&st).await {
                    Some(provider) => {
                        let settings = st.effective_settings();
                        if let Err(e) = run_llm_curator(st.clone(), provider, &settings).await {
                            log::warn!("curator: llm pass failed: {e:#}");
                        }
                    }
                    None => log::info!("curator: skipped llm pass (no provider)"),
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_due_when_never_run() {
        let settings = ModelSettings {
            curator_interval_days: 7,
            ..ModelSettings::default()
        };
        assert!(llm_curator_due(&settings));
    }
}
