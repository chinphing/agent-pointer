//! Workspace resolution when a lead agent delegates to a sub-agent (especially **coder**).

use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use std::path::{Path, PathBuf};

use crate::models::ModelSettings;
use crate::session_sandbox::SessionSandbox;

/// Resolve workspace for a **`run_subagent`** call and write it to `settings.workspace_root`.
///
/// Priority: explicit tool arg → existing `settings.workspace_root` → session sandbox.
/// Returns whether the resolved path is an ephemeral session sandbox.
pub fn ensure_subagent_workspace(
    conversation_id: &str,
    explicit_from_tool: Option<&str>,
    settings: &mut ModelSettings,
) -> Result<bool> {
    let (root, ephemeral) = resolve_subagent_workspace(
        conversation_id,
        explicit_from_tool,
        settings.workspace_root.as_str(),
    )?;
    settings.workspace_root = root;
    Ok(ephemeral)
}

fn resolve_subagent_workspace(
    conversation_id: &str,
    explicit_from_tool: Option<&str>,
    session_workspace: &str,
) -> Result<(String, bool)> {
    if let Some(raw) = explicit_from_tool.map(str::trim).filter(|s| !s.is_empty()) {
        let path = resolve_explicit_workspace_root(raw)?;
        info!(
            "workspace_delegation: using explicit path for conversation_id={conversation_id}: {path}"
        );
        return Ok((path, false));
    }

    let session_ws = session_workspace.trim();
    if !session_ws.is_empty() {
        return match resolve_explicit_workspace_root(session_ws) {
            Ok(path) => {
                let ephemeral = SessionSandbox::is_sandbox(Path::new(&path)).unwrap_or(false);
                info!(
                    "workspace_delegation: using session workspace for conversation_id={conversation_id}: {path} ephemeral={ephemeral}"
                );
                Ok((path, ephemeral))
            }
            Err(_) if SessionSandbox::is_sandbox(Path::new(session_ws)).unwrap_or(false) => {
                std::fs::create_dir_all(session_ws).with_context(|| {
                    format!("workspace_delegation: create session sandbox failed: {session_ws}")
                })?;
                let path = resolve_explicit_workspace_root(session_ws)?;
                warn!(
                    "workspace_delegation: created missing session sandbox conversation_id={conversation_id}: {path}"
                );
                Ok((path, true))
            }
            Err(e) => Err(e),
        };
    }

    let uid = session_user_id_for_conversation(conversation_id);
    let sandbox = SessionSandbox::ensure_default(conversation_id, uid.as_str())
        .map(|p| p.display().to_string())?;
    info!(
        "workspace_delegation: ephemeral sandbox for conversation_id={conversation_id}: {sandbox}"
    );
    Ok((sandbox, true))
}

fn session_user_id_for_conversation(conversation_id: &str) -> String {
    crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.session_user_id(conversation_id).ok())
        .unwrap_or_default()
}

/// Validate and canonicalize an explicit `workspaceRoot` from a subagent call.
///
/// Registered workers and self forks share this policy: an explicit directory
/// overrides the parent session workspace.
pub fn resolve_explicit_workspace_root(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let path = if trimmed.starts_with('~') {
        crate::media::access::expand_root(trimmed)?
    } else {
        PathBuf::from(trimmed)
    };
    if !path.is_absolute() {
        return Err(anyhow!("workspace path must be absolute: {raw}"));
    }
    if !path.exists() {
        return Err(anyhow!("workspace path does not exist: {raw}"));
    }
    if !path.is_dir() {
        return Err(anyhow!("workspace path is not a directory: {raw}"));
    }
    path.canonicalize()
        .map(|p| p.display().to_string())
        .with_context(|| format!("cannot canonicalize workspace path: {raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn settings_with_workspace(ws: &str) -> ModelSettings {
        let mut s = ModelSettings::default();
        s.workspace_root = ws.to_string();
        s
    }

    #[test]
    fn explicit_relative_path_rejected() {
        assert!(resolve_explicit_workspace_root("relative/path").is_err());
    }

    #[test]
    fn ensure_prefers_explicit_over_session() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().display().to_string();
        let session = tempfile::tempdir().unwrap();
        let mut settings = settings_with_workspace(session.path().to_str().unwrap());
        let ephemeral =
            ensure_subagent_workspace("conv_explicit", Some(&explicit), &mut settings).unwrap();
        assert_eq!(
            settings.workspace_root,
            dir.path().canonicalize().unwrap().display().to_string()
        );
        assert!(!ephemeral);
    }

    #[test]
    fn ensure_uses_session_workspace_when_set() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().display().to_string();
        let mut settings = settings_with_workspace(&ws);
        let ephemeral = ensure_subagent_workspace("conv_session", None, &mut settings).unwrap();
        assert_eq!(
            settings.workspace_root,
            dir.path().canonicalize().unwrap().display().to_string()
        );
        assert!(!ephemeral);
    }

    #[test]
    fn ensure_falls_back_to_session_sandbox() {
        let mut settings = settings_with_workspace("");
        let ephemeral =
            ensure_subagent_workspace("conv_fallback_test", None, &mut settings).unwrap();
        assert!(settings.workspace_root.contains("session-sandboxes"));
        assert!(settings.workspace_root.contains("_anonymous"));
        assert!(settings.workspace_root.contains("conv_fallback_test"));
        assert!(Path::new(&settings.workspace_root).is_dir());
        assert!(ephemeral);
    }

    #[test]
    fn ensure_recreates_missing_user_session_sandbox() {
        let user_id = format!("test-user-{}", uuid::Uuid::new_v4());
        let stale = SessionSandbox::default_path("conv_new_chat", &user_id)
            .unwrap()
            .display()
            .to_string();
        assert!(!Path::new(&stale).exists());
        let mut settings = settings_with_workspace(&stale);
        let ephemeral = ensure_subagent_workspace("conv_new_chat", None, &mut settings).unwrap();
        assert!(Path::new(&settings.workspace_root).is_dir());
        assert_eq!(
            settings.workspace_root,
            Path::new(&stale)
                .canonicalize()
                .unwrap()
                .display()
                .to_string()
        );
        assert!(ephemeral);
    }

    #[test]
    fn ensure_recreates_missing_legacy_session_sandbox() {
        let conv_id = format!("conv_stale_{}", uuid::Uuid::new_v4());
        let stale = SessionSandbox::legacy_conversation_path(&conv_id)
            .unwrap()
            .display()
            .to_string();
        assert!(!Path::new(&stale).exists());
        let mut settings = settings_with_workspace(&stale);
        let ephemeral = ensure_subagent_workspace(&conv_id, None, &mut settings).unwrap();
        assert!(Path::new(&settings.workspace_root).is_dir());
        assert_eq!(
            settings.workspace_root,
            Path::new(&stale)
                .canonicalize()
                .unwrap()
                .display()
                .to_string()
        );
        assert!(ephemeral);
    }
}
