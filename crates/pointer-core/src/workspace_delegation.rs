//! Workspace resolution when a lead agent delegates to **coder** (general has no picker).

use anyhow::{anyhow, Context, Result};
use log::info;
use std::path::PathBuf;

use crate::storage;
use crate::tools::file::{resolve_tool_workspace_root, set_runtime_workspace_root, workspace_root_from_override_or_settings};

const CODER_SANDBOXES_DIR: &str = "coder-sandboxes";

/// Resolve workspace for a **coder** `run_subagent` call.
///
/// Priority: explicit tool arg → valid in-run / settings root → per-conversation sandbox.
/// Returns `(absolute_path, is_ephemeral_sandbox)`.
pub fn ensure_coder_delegation_workspace(
    conversation_id: &str,
    explicit_from_tool: Option<&str>,
) -> Result<(String, bool)> {
    if let Some(raw) = explicit_from_tool.map(str::trim).filter(|s| !s.is_empty()) {
        let path = validate_existing_workspace_dir(raw)?;
        set_runtime_workspace_root(path.clone());
        info!(
            "workspace_delegation: using explicit path for conversation_id={conversation_id}: {path}"
        );
        return Ok((path, false));
    }

    let current = workspace_root_from_override_or_settings();
    if !current.trim().is_empty() {
        match resolve_tool_workspace_root() {
            Ok(p) => {
                let path = p.display().to_string();
                set_runtime_workspace_root(path.clone());
                info!(
                    "workspace_delegation: using session workspace for conversation_id={conversation_id}: {path}"
                );
                return Ok((path, false));
            }
            Err(e) => {
                log::warn!(
                    "workspace_delegation: session workspace invalid for conversation_id={conversation_id}: {e:#}; using sandbox"
                );
            }
        }
    }

    let sandbox = ensure_conversation_sandbox(conversation_id)?;
    set_runtime_workspace_root(sandbox.clone());
    info!(
        "workspace_delegation: ephemeral sandbox for conversation_id={conversation_id}: {sandbox}"
    );
    Ok((sandbox, true))
}

fn validate_existing_workspace_dir(raw: &str) -> Result<String> {
    let path = PathBuf::from(raw.trim());
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

fn ensure_conversation_sandbox(conversation_id: &str) -> Result<String> {
    let cid = conversation_id.trim();
    if cid.is_empty() {
        return Err(anyhow!("conversation id is empty"));
    }
    if cid.contains('/') || cid.contains('\\') || cid.contains('\0') {
        return Err(anyhow!("invalid conversation id for sandbox"));
    }
    let base = storage::app_data_dir()?.join(CODER_SANDBOXES_DIR).join(cid);
    std::fs::create_dir_all(&base).with_context(|| {
        format!(
            "failed to create coder sandbox directory: {}",
            base.display()
        )
    })?;
    Ok(base
        .canonicalize()
        .unwrap_or(base)
        .display()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::file::ConversationWorkspaceGuard;
    use std::path::Path;

    #[test]
    fn sandbox_path_is_under_app_data() {
        let path = ensure_conversation_sandbox("conv_test_123").unwrap();
        assert!(path.contains("coder-sandboxes"));
        assert!(path.contains("conv_test_123"));
        assert!(Path::new(&path).is_dir());
    }

    #[test]
    fn explicit_relative_path_rejected() {
        assert!(validate_existing_workspace_dir("relative/path").is_err());
    }

    #[test]
    fn ensure_prefers_explicit_over_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().display().to_string();
        let (path, ephemeral) =
            ensure_coder_delegation_workspace("conv_explicit", Some(&explicit)).unwrap();
        assert_eq!(path, dir.path().canonicalize().unwrap().display().to_string());
        assert!(!ephemeral);
        let _guard = ConversationWorkspaceGuard::enter(String::new());
    }
}
