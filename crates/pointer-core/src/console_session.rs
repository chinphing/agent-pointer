//! Persistent user-controlled PTY sessions for the workspace console.
//!
//! This deliberately stays separate from the Agent `terminal` tool: every
//! console tab owns an interactive shell and may outlive an individual chat turn.

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use portable_pty::PtySize;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

use crate::console_term_query::TermQueryFilter;
use crate::models::StreamEvent;
use crate::stream_broadcast::publish_global_stream;

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const MAX_ROWS: u16 = 1_000;
const MAX_COLS: u16 = 1_000;

/// Serializable console-tab metadata returned to either frontend host.
#[derive(Debug, Clone, Serialize)]
pub struct ConsoleSessionInfo {
    pub id: String,
    #[serde(rename = "workspaceRoot")]
    pub workspace_root: String,
    pub cwd: String,
    pub label: String,
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
}

struct ConsoleSession {
    info: ConsoleSessionInfo,
    /// 交互式终端句柄：宿主走 portable_pty，沙箱走 envd 的 pty，上层无感。
    pty: Box<dyn crate::env::PtyHandle>,
}

impl ConsoleSession {
    fn emit_output(&self, output: String) {
        if !output.is_empty() {
            publish_global_stream(StreamEvent::ConsoleOutputDelta {
                session_id: self.info.id.clone(),
                workspace_root: self.info.workspace_root.clone(),
                cwd: self.info.cwd.clone(),
                output,
            });
        }
    }
}

/// In-memory registry of independent, user-owned shell tabs.
///
/// Unlike the Agent terminal's one-shot process map, no workspace-level
/// de-duplication occurs: every call to `create` spawns a distinct PTY.
#[derive(Default)]
pub struct ConsoleSessionManager {
    sessions: Mutex<HashMap<String, Arc<ConsoleSession>>>,
}

impl ConsoleSessionManager {
    /// 开一个终端标签。
    ///
    /// `session_user_id` = **当前请求身份**（服务端：登录用户优先、standalone 回落
    /// 本地操作者；桌面端同一规则）。档位只按这个身份解析；会话行 uid 仅用于校验
    /// 「这个会话是不是你的」——规则见 [`resolve_console_user_id`]。
    ///
    /// `request_is_admin` = 当前请求视角是否管理员（[`crate::chat_service::AppState::is_admin_session`]），
    /// 与 Web 入口的 `conversation_access_decision` 同口径：管理员可以打开别人的会话，
    /// 但档位仍按**请求身份**解析（不是按会话主人）。
    pub fn create(
        &self,
        workspace_root: &str,
        conversation_id: &str,
        session_user_id: &str,
        request_is_admin: bool,
        cwd: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<ConsoleSessionInfo> {
        let stored = stored_conversation_user_id(conversation_id);
        self.create_for_owner(
            workspace_root,
            conversation_id,
            session_user_id,
            stored.as_deref(),
            request_is_admin,
            cwd,
            cols,
            rows,
        )
    }

    /// 归属信息已查出时开终端（`stored_user_id` 来自 [`stored_conversation_user_id`]）。
    // 参数与 [`Self::create`] 一一对应，只是把归属查询拆出来——单测能直接喂会话行 uid。
    #[allow(clippy::too_many_arguments)]
    fn create_for_owner(
        &self,
        workspace_root: &str,
        conversation_id: &str,
        request_user_id: &str,
        stored_user_id: Option<&str>,
        request_is_admin: bool,
        cwd: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<ConsoleSessionInfo> {
        // 先过归属校验：不符就直接返回，既不起沙箱，也不在宿主执行。
        let user_id = resolve_console_user_id(
            conversation_id,
            request_user_id,
            stored_user_id,
            request_is_admin,
        )?;
        // 执行环境按**请求身份**解析：沙箱用户开沙箱终端，admin 开宿主终端。
        // 注意不能只查缓存——应用刚重启、或这个会话还没跑过对话时缓存是空的。
        // 档位解析不出来时明确报错，不按部署档位兜底（那会把终端静默开到宿主上，
        // 或者给沙箱用户一个 `DeniedEnv` 让它误报"不支持交互式终端"）。
        let env = console_env(conversation_id, Path::new(workspace_root), &user_id)?;
        self.open_session(env, workspace_root, conversation_id, cwd, cols, rows)
    }

    /// 环境已确定时开终端（身份与档位解析由 [`Self::create_for_owner`] 负责）。
    fn open_session(
        &self,
        env: Arc<dyn crate::env::Environment>,
        workspace_root: &str,
        conversation_id: &str,
        cwd: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<ConsoleSessionInfo> {
        let workspace_root = normalize_directory_in(env.as_ref(), workspace_root, "工作区目录")?;
        let cwd = match cwd.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => normalize_directory_in(env.as_ref(), value, "终端目录")?,
            None => workspace_root.clone(),
        };
        let info = ConsoleSessionInfo {
            id: Uuid::new_v4().to_string(),
            workspace_root,
            conversation_id: conversation_id.to_string(),
            label: directory_label(&cwd),
            cwd,
        };
        let session = Arc::new(spawn_console_session(env, info, cols, rows)?);
        let response = session.info.clone();
        self.sessions
            .lock()
            .insert(response.id.clone(), session.clone());
        spawn_output_reader(session);
        Ok(response)
    }

    pub fn write(&self, session_id: &str, data: &str) -> Result<()> {
        let session = self.session(session_id)?;
        session.pty.write(data.as_bytes())
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let session = self.session(session_id)?;
        let size = normalize_size(cols, rows);
        session.pty.resize(size.cols, size.rows)
    }

    pub fn close(&self, session_id: &str) -> bool {
        let Some(session) = self.sessions.lock().remove(session_id) else {
            return false;
        };
        if let Err(error) = session.pty.kill() {
            log::warn!("console: failed to stop session {session_id}: {error}");
        }
        publish_global_stream(StreamEvent::ConsoleSessionExited {
            session_id: session.info.id.clone(),
            workspace_root: session.info.workspace_root.clone(),
            cwd: session.info.cwd.clone(),
            exit_code: None,
        });
        true
    }

    pub fn close_all(&self) {
        let ids: Vec<String> = self.sessions.lock().keys().cloned().collect();
        for id in ids {
            self.close(&id);
        }
    }

    /// List console tabs that belong to one conversation.
    pub fn list_for_conversation(&self, conversation_id: &str) -> Vec<ConsoleSessionInfo> {
        let cid = conversation_id.trim();
        self.sessions
            .lock()
            .values()
            .filter(|s| s.info.conversation_id == cid)
            .map(|s| s.info.clone())
            .collect()
    }

    /// Close every console tab for a conversation (session delete / hard cancel).
    pub fn close_for_conversation(&self, conversation_id: &str) -> usize {
        let ids: Vec<String> = self
            .list_for_conversation(conversation_id)
            .into_iter()
            .map(|info| info.id)
            .collect();
        let mut closed = 0;
        for id in ids {
            if self.close(&id) {
                closed += 1;
            }
        }
        if closed > 0 {
            log::info!(
                "console: closed {closed} session(s) for conversation={conversation_id}"
            );
        }
        closed
    }

    fn session(&self, session_id: &str) -> Result<Arc<ConsoleSession>> {
        self.sessions
            .lock()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow!("终端会话不存在或已关闭"))
    }
}

/// 会话行里记录的归属用户（`conversations.session_user_id`）。
///
/// 查不到（无这一行 / 读库失败 / 空串）都返回 `None`——历史会话没有归属是正常情况，
/// 此时**不**据此提权，也不据此拒绝，档位一律按请求身份解析。
fn stored_conversation_user_id(conversation_id: &str) -> Option<String> {
    let stored = crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.session_user_id(conversation_id).ok());
    non_empty_uid(stored.as_deref())
}

/// 本次终端会话用哪个身份解析档位，并校验会话归属。
///
/// 档位的语义是「这个执行者能拿到什么环境」，不是「这个会话属于谁」，所以：
///
/// - 请求身份（`request_user_id`）为空 → **拒绝**：拿不到身份反而能拿到宿主执行权限
///   是提权路径，宁可不开终端（也不回落到会话行 uid，那等于替别人执行）。
/// - 会话行 uid 非空且 != 请求身份 → 普通用户**拒绝**：会话 id 是客户端可传的，B 传
///   A 的会话 id 就能借 A 的档位拿到宿主 shell。管理员**放行**（代操作是既有能力，
///   与 Web 入口 `conversation_access_decision` 同口径），但档位仍按请求身份解析。
/// - 会话行 uid 为空（历史会话 / 没有这一行）→ 放行，仍按请求身份解析档位，
///   不会因为「归属为空」而提权。
/// - 会话行 uid == 请求身份 → 放行。
///
/// 放行只影响「允不允许开」，不影响「以谁的档位执行」：返回值永远是请求身份。
fn resolve_console_user_id(
    conversation_id: &str,
    request_user_id: &str,
    stored_user_id: Option<&str>,
    request_is_admin: bool,
) -> Result<String> {
    let Some(request_uid) = non_empty_uid(Some(request_user_id)) else {
        return Err(anyhow!(
            "无法确定当前用户身份，已拒绝创建终端会话（既不在宿主执行，也不创建沙箱）：{conversation_id}"
        ));
    };
    if let Some(stored_uid) = non_empty_uid(stored_user_id) {
        if stored_uid != request_uid {
            if !request_is_admin {
                return Err(anyhow!(
                    "会话不属于当前用户，已拒绝创建终端会话：{conversation_id}"
                ));
            }
            log::info!(
                "console: admin override conversation_id={conversation_id} owner={stored_uid} \
                 actor={request_uid}"
            );
        }
    }
    Ok(request_uid)
}

fn non_empty_uid(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|uid| !uid.is_empty())
        .map(str::to_string)
}

/// 终端该用哪个环境。
///
/// 顺序：按**请求身份**解析档位 → 档位相符的缓存复用 → 按档位新建。
/// `user_id` 由 [`resolve_console_user_id`] 保证非空——**绝不**用空 uid 解析档位
/// （那会跳过 admin 判定、直接落到部署默认档位，沙箱部署下等于给 admin 建了一个沙箱）。
fn console_env(
    conversation_id: &str,
    workspace_root: &Path,
    user_id: &str,
) -> Result<Arc<dyn crate::env::Environment>> {
    match crate::env::provider::ProviderConfig::from_env() {
        Ok(cfg) => console_env_with(&cfg, conversation_id, workspace_root, user_id),
        Err(error) => {
            // 读不出配置就不猜：先沿用该会话已建好的环境；没有就**明确拒绝**，
            // 不按部署档位兜底——local 部署下兜底等于把终端静默开到宿主上。
            log::warn!("console: 读取执行环境配置失败: {error:#}");
            crate::env::session_env::env_for_conversation(conversation_id).ok_or_else(|| {
                anyhow!("无法解析终端执行环境档位，已拒绝创建终端会话：{error:#}")
            })
        }
    }
}

/// 已建好的环境能否给这个档位复用。
///
/// 缓存条目是**上一个执行者**绑的（会话行 uid 为空的历史会话，别的身份也能打开它），
/// 直接照抄等于借别人的环境——档位不同就不能复用。
fn cached_env_matches_plan(plan: ConsoleEnvPlan, kind: crate::env::EnvKind) -> bool {
    match plan {
        ConsoleEnvPlan::Host => kind == crate::env::EnvKind::Local,
        ConsoleEnvPlan::Sandbox => kind != crate::env::EnvKind::Local,
        // 档位读不出来：沿用已有环境，不新建容器（与 `effective_env()` 兜底一致）。
        ConsoleEnvPlan::Fallback => true,
    }
}

/// 按给定配置解析终端环境（单测直接喂配置，不起容器）。
fn console_env_with(
    cfg: &crate::env::provider::ProviderConfig,
    conversation_id: &str,
    workspace_root: &Path,
    user_id: &str,
) -> Result<Arc<dyn crate::env::Environment>> {
    let plan = console_env_plan(cfg, conversation_id, user_id);
    if let Some(env) = crate::env::session_env::env_for_conversation(conversation_id) {
        if cached_env_matches_plan(plan, env.kind()) {
            return Ok(env);
        }
        log::warn!(
            "console: 会话 {conversation_id} 已缓存的环境与当前用户档位不符，按当前档位重开"
        );
    }
    match plan {
        ConsoleEnvPlan::Host => crate::env::session_env::ensure_local_session(
            conversation_id,
            user_id,
            workspace_root,
        ),
        // 档位解析失败：不猜，也不按部署档位兜底（见 [`console_env_plan`] 的日志）。
        ConsoleEnvPlan::Fallback => Err(anyhow!(
            "无法解析会话 {conversation_id} 的执行环境档位，已拒绝创建终端会话"
        )),
        ConsoleEnvPlan::Sandbox => {
            match crate::env::session_env::resolve_session_env(
                cfg,
                conversation_id,
                user_id,
                Some(workspace_root),
            ) {
                Ok(Some(env)) => Ok(env),
                // 判定说沙箱、解析却是 local 档位：按 local 处理。
                Ok(None) => crate::env::session_env::ensure_local_session(
                    conversation_id,
                    user_id,
                    workspace_root,
                ),
                Err(error) => {
                    // 沙箱建不出来也不能静默跑宿主：明确报错，别让用户看到
                    // 一个 `DeniedEnv` 吐出来的「工作区目录无效」。
                    log::warn!(
                        "console: 会话 {conversation_id} 的沙箱不可用: {error:#}"
                    );
                    Err(anyhow!(
                        "会话 {conversation_id} 的沙箱不可用，已拒绝创建终端会话：{error:#}"
                    ))
                }
            }
        }
    }
}

/// 归属用户已知时，终端走宿主还是沙箱。
///
/// 只做档位判定，**不建沙箱**：单测能断言 admin / 普通用户的走向而不起容器
/// （真建容器由 `tests/desktop_tier_it.rs` 覆盖）。
fn console_env_plan(
    cfg: &crate::env::provider::ProviderConfig,
    conversation_id: &str,
    user_id: &str,
) -> ConsoleEnvPlan {
    match cfg.resolve(Some(user_id)) {
        Ok(resolved) if resolved.tier != crate::env::provider::Tier::Local => {
            ConsoleEnvPlan::Sandbox
        }
        Ok(_) => ConsoleEnvPlan::Host,
        Err(error) => {
            log::warn!("console: 解析会话 {conversation_id} 的执行环境档位失败: {error:#}");
            ConsoleEnvPlan::Fallback
        }
    }
}

/// 终端环境走向（判定结果，不含已建好的环境）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsoleEnvPlan {
    /// 宿主环境：admin 名单 / local 档位。
    Host,
    /// 非 local 档位：按会话建 / 复用沙箱。
    Sandbox,
    /// 档位解析失败：由调用方明确拒绝开终端（不按部署档位兜底）。
    Fallback,
}

impl Drop for ConsoleSessionManager {
    fn drop(&mut self) {
        let sessions = self.sessions.get_mut();
        for session in sessions.values() {
            if let Err(error) = session.pty.kill() {
                log::debug!("console: failed to stop session during cleanup: {error}");
            }
        }
    }
}

fn normalize_directory(input: &str, description: &str) -> Result<String> {
    let value = input.trim();
    if value.is_empty() {
        return Err(anyhow!("请先选择{description}"));
    }
    let path = Path::new(value);
    if !path.is_dir() {
        return Err(anyhow!("{description}不存在: {value}"));
    }
    let canonical = path.canonicalize()?.to_string_lossy().to_string();
    // Rust canonicalize() on Windows returns the `\\?\`-prefixed extended
    // path form. Passing that as a shell cwd makes PowerShell render the
    // prompt as `PS Microsoft.PowerShell.Core\FileSystem::\\?\C:\...`.
    // Strip the prefix so the shell and UI see a normal `C:\...` path.
    #[cfg(windows)]
    let canonical = canonical.trim_start_matches(r"\\?\").to_string();
    Ok(canonical)
}

/// 环境感知的目录校验：宿主用 `is_dir` + canonicalize，沙箱用 env.metadata。
///
/// 沙箱会话里前端传的仍是宿主工作区路径，在沙箱里不存在——这种情况下退回
/// 环境工作区根（`/workspace`），而不是报错或静默去开宿主目录。
fn normalize_directory_in(
    env: &dyn crate::env::Environment,
    input: &str,
    description: &str,
) -> Result<String> {
    if env.kind() == crate::env::EnvKind::Local {
        return normalize_directory(input, description);
    }
    let guest_root = env.workspace_root().display().to_string();
    let value = input.trim();
    if value.is_empty() {
        return Ok(guest_root);
    }
    let path = Path::new(value);
    // 已经是沙箱内路径（存在）→ 直接用；否则（宿主路径 / 空）→ 用工作区根。
    match env.metadata(path) {
        Ok(meta) if meta.is_dir => Ok(value.to_string()),
        _ => Ok(guest_root),
    }
}

fn directory_label(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

fn normalize_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols: if cols == 0 {
            DEFAULT_COLS
        } else {
            cols.min(MAX_COLS)
        },
        rows: if rows == 0 {
            DEFAULT_ROWS
        } else {
            rows.min(MAX_ROWS)
        },
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn spawn_console_session(
    env: Arc<dyn crate::env::Environment>,
    info: ConsoleSessionInfo,
    cols: u16,
    rows: u16,
) -> Result<ConsoleSession> {
    let size = normalize_size(cols, rows);
    let mut spec = crate::env::PtySpec::new(shell_argv(env.as_ref()));
    spec.cwd = Some(PathBuf::from(&info.cwd));
    spec.cols = size.cols;
    spec.rows = size.rows;
    // 宿主沿用用户环境；沙箱只带最小可用集合，避免把宿主 PATH 带进去。
    if env.kind() == crate::env::EnvKind::Local {
        spec.env = crate::dotenv::build_terminal_child_environment(&[])
            .into_iter()
            .collect();
    }
    spec.env.push(("TERM".to_string(), "xterm-256color".to_string()));
    if env.kind() != crate::env::EnvKind::Local {
        spec.env.push(("LANG".to_string(), "C.UTF-8".to_string()));
    }
    let pty = env.pty_open(spec)?;
    Ok(ConsoleSession { info, pty })
}

/// 交互式 shell：宿主按用户 `$SHELL`，沙箱固定 bash（镜像里一定有）。
fn shell_argv(env: &dyn crate::env::Environment) -> Vec<String> {
    if env.kind() != crate::env::EnvKind::Local {
        return vec!["/bin/bash".to_string(), "-l".to_string()];
    }
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|value| Path::new(value).is_file())
            .unwrap_or_else(|| "sh".to_string());
        vec![shell, "-l".to_string()]
    }
    #[cfg(windows)]
    {
        vec!["powershell".to_string(), "-NoLogo".to_string()]
    }
}

fn write_pty_replies(session: &ConsoleSession, replies: &[Vec<u8>]) {
    if replies.is_empty() {
        return;
    }
    for reply in replies {
        if let Err(error) = session.pty.write(reply) {
            log::warn!(
                "console: failed to write term-query reply session={}: {error}",
                session.info.id
            );
            return;
        }
    }
}

fn spawn_output_reader(session: Arc<ConsoleSession>) {
    std::thread::spawn(move || {
        let Ok(mut reader) = session.pty.take_reader() else {
            log::warn!("console: failed to take PTY reader");
            return;
        };
        // Answer OSC/DA/CPR beside the PTY so replies never round-trip through xterm.js.
        let mut query_filter = TermQueryFilter::default();
        let mut carry = Vec::new();
        let mut forward = Vec::new();
        let mut replies = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => {
                    forward.clear();
                    replies.clear();
                    query_filter.process(&buf[..count], &mut forward, &mut replies);
                    write_pty_replies(&session, &replies);
                    if !forward.is_empty() {
                        let output =
                            crate::windows_shell_encoding::decode_utf8_stream(&mut carry, &forward);
                        session.emit_output(output);
                    }
                }
                Err(error) => {
                    log::debug!("console: PTY reader ended: {error}");
                    break;
                }
            }
        }
        forward.clear();
        query_filter.finish(&mut forward);
        if !forward.is_empty() {
            let output = crate::windows_shell_encoding::decode_utf8_stream(&mut carry, &forward);
            session.emit_output(output);
        }
        let tail = crate::windows_shell_encoding::decode_utf8_finish(&mut carry);
        session.emit_output(tail);
        // 读完输出即视为会话结束：拿一次退出码（沙箱侧由 end 事件回填）。
        let exit_code = session.pty.try_wait().ok().flatten();
        publish_global_stream(StreamEvent::ConsoleSessionExited {
            session_id: session.info.id.clone(),
            workspace_root: session.info.workspace_root.clone(),
            cwd: session.info.cwd.clone(),
            exit_code,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::ConsoleSessionManager;
    use super::{
        cached_env_matches_plan, console_env_plan, console_env_with, normalize_directory,
        resolve_console_user_id, spawn_console_session, ConsoleEnvPlan, ConsoleSessionInfo,
    };
    use std::io::{Read, Write};
    use std::path::Path;
    use std::sync::Arc;

    /// 显式宿主环境：单测不依赖部署档位配置（真实解析见 `console_env_plan` 用例）。
    fn host_env(root: &Path) -> Arc<dyn crate::env::Environment> {
        Arc::new(crate::env::LocalEnv::new(root.to_path_buf()))
    }

    /// 沙箱部署配置（desktop + docker），`[auth] admins = ["admin"]`。
    fn sandbox_deployment_cfg() -> crate::env::provider::ProviderConfig {
        crate::env::provider::ProviderConfig {
            tier: Some(crate::env::provider::Tier::Desktop),
            backend: Some(crate::env::provider::Backend::Docker),
            admins: vec!["admin".into()],
            platform: true,
            ..Default::default()
        }
    }

    /// 请求身份缺失 → 拒绝：既不起沙箱，也不在宿主执行。
    #[test]
    fn console_session_refuses_unknown_user() {
        let root = tempfile::tempdir().expect("workspace");
        let manager = ConsoleSessionManager::default();
        // 唯一 id：会话行里查不到，请求身份也传空串。
        let conversation = format!("conv-no-identity-{}", uuid::Uuid::new_v4());
        let error = manager
            .create(
                root.path().to_str().expect("utf8 path"),
                &conversation,
                "",
                false,
                None,
                80,
                24,
            )
            .expect_err("身份缺失必须拒绝创建终端会话");
        let message = error.to_string();
        assert!(message.contains("无法确定当前用户身份"), "{message}");
        assert!(
            manager.sessions.lock().is_empty(),
            "拒绝时不能留下终端会话（不建沙箱，也不走宿主）"
        );
        assert!(
            crate::env::session_env::env_for_conversation_peek(&conversation).is_none(),
            "拒绝时不能给这个会话建出任何环境（沙箱 / 宿主都不行）"
        );
    }

    /// 请求身份缺失时**不能**拿会话行 uid 顶上：那等于替会话主人执行（提权）。
    #[test]
    fn missing_request_identity_does_not_borrow_conversation_owner() {
        let error = resolve_console_user_id("conv-owner-admin", "", Some("admin"), false)
            .expect_err("请求身份缺失必须拒绝");
        assert!(error.to_string().contains("无法确定当前用户身份"), "{error}");
    }

    /// 提权回归：普通用户传别人的会话 id → 拒绝，且什么都不建（不建沙箱、不走宿主）。
    #[test]
    fn foreign_conversation_is_rejected_before_spawning() {
        let root = tempfile::tempdir().expect("workspace");
        let manager = ConsoleSessionManager::default();
        let conversation = format!("conv-admin-owned-{}", uuid::Uuid::new_v4());
        let error = manager
            .create_for_owner(
                root.path().to_str().expect("utf8 path"),
                &conversation,
                "alice",
                Some("admin"),
                false,
                None,
                80,
                24,
            )
            .expect_err("会话归属不符必须拒绝");
        let message = error.to_string();
        assert!(message.contains("不属于当前用户"), "{message}");
        assert!(
            manager.sessions.lock().is_empty(),
            "拒绝时不能留下终端会话（不建沙箱，也不走宿主）"
        );
        assert!(
            crate::env::session_env::env_for_conversation_peek(&conversation).is_none(),
            "拒绝时不能给这个会话建出任何环境（沙箱 / 宿主都不行）"
        );
    }

    /// 管理员打开别人的会话：放行（与 Web 入口 `conversation_access_decision` 同口径），
    /// 但档位仍按**请求身份**解析——不是按会话主人。
    #[test]
    fn admin_can_open_another_users_console_with_own_tier() {
        let cfg = sandbox_deployment_cfg();
        let conversation = "conv-alice-owned-console";
        assert_eq!(
            resolve_console_user_id(conversation, "admin", Some("alice"), true)
                .expect("管理员可以打开他人会话"),
            "admin"
        );
        // 档位按 admin 自己解析 → 宿主，而不是 alice 的沙箱。
        assert_eq!(
            console_env_plan(&cfg, conversation, "admin"),
            ConsoleEnvPlan::Host
        );
        // 同一份输入换成普通用户 → 仍然拒绝。
        let error = resolve_console_user_id(conversation, "bob", Some("alice"), false)
            .expect_err("普通用户不能打开他人会话");
        assert!(error.to_string().contains("不属于当前用户"), "{error}");
        // 请求身份缺失时管理员标记救不了：fail closed 不变。
        assert!(resolve_console_user_id(conversation, "", Some("alice"), true).is_err());
    }

    /// 会话归属为空（历史会话 / 没有这一行）→ 放行，但档位仍按请求身份解析：
    /// 普通用户不会被当成 admin，也不会复用别人留下的宿主环境。
    #[test]
    fn empty_conversation_owner_does_not_escalate() {
        let cfg = sandbox_deployment_cfg();
        let conversation = "conv-legacy-no-owner";
        assert_eq!(
            resolve_console_user_id(conversation, "alice", Some(""), false).expect("空归属放行"),
            "alice"
        );
        assert_eq!(
            resolve_console_user_id(conversation, "alice", None, false).expect("没有会话行也放行"),
            "alice"
        );
        // 档位只认请求身份：普通用户 → 沙箱，不是宿主。
        assert_eq!(
            console_env_plan(&cfg, conversation, "alice"),
            ConsoleEnvPlan::Sandbox
        );
        // 历史会话里别人（admin）留下的宿主环境不能复用。
        assert!(
            !cached_env_matches_plan(ConsoleEnvPlan::Sandbox, crate::env::EnvKind::Local),
            "沙箱档位不能复用宿主环境"
        );
        assert!(
            cached_env_matches_plan(ConsoleEnvPlan::Sandbox, crate::env::EnvKind::Desktop),
            "沙箱档位应复用同一沙箱环境"
        );
        assert!(
            cached_env_matches_plan(ConsoleEnvPlan::Host, crate::env::EnvKind::Local),
            "宿主档位复用宿主环境"
        );
        assert!(
            !cached_env_matches_plan(ConsoleEnvPlan::Host, crate::env::EnvKind::Headless),
            "宿主档位不能复用沙箱环境"
        );
    }

    /// 归属与请求身份一致 → 放行，档位按该身份解析（admin 宿主 / 普通用户沙箱）。
    #[test]
    fn owned_conversation_resolves_by_request_identity() {
        let cfg = sandbox_deployment_cfg();
        assert_eq!(
            resolve_console_user_id("conv-admin-own", "admin", Some("admin"), false)
                .expect("本人会话放行"),
            "admin"
        );
        assert_eq!(
            console_env_plan(&cfg, "conv-admin-own", "admin"),
            ConsoleEnvPlan::Host
        );
        assert_eq!(
            resolve_console_user_id("conv-alice-own", "alice", Some("alice"), false)
                .expect("本人会话放行"),
            "alice"
        );
        assert_eq!(
            console_env_plan(&cfg, "conv-alice-own", "alice"),
            ConsoleEnvPlan::Sandbox
        );
        // 空白差异按规范化后比较（`normalize_session_user_id` 只 trim）。
        assert!(resolve_console_user_id("conv-alice-own", " alice ", Some("alice "), false).is_ok());
    }

    /// 回归：以前用空 uid 解析档位，admin 会被判成部署默认档位 → 被建沙箱。
    #[test]
    fn admin_console_env_is_host_not_sandbox() {
        let _lock = crate::storage::test_app_data_dir_lock();
        let app = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(app.path().to_path_buf());
        let cfg = sandbox_deployment_cfg();
        let dir = tempfile::tempdir().unwrap();
        let conversation = format!("conv-admin-console-{}", uuid::Uuid::new_v4());
        assert_eq!(
            console_env_plan(&cfg, &conversation, "admin"),
            ConsoleEnvPlan::Host
        );
        let env = console_env_with(&cfg, &conversation, dir.path(), "admin")
            .expect("local 档位必须给用户环境");
        assert_eq!(env.kind(), crate::env::EnvKind::Local);
        assert!(
            !env.path_map().is_host_passthrough(),
            "admin 的终端也不能使用整机文件视图"
        );
        assert!(env.workspace_root().starts_with(env.data_root()));
        crate::env::session_env::release(&conversation);
        crate::storage::clear_test_app_data_dir();
    }

    /// 同一份配置下普通用户仍是沙箱档位——身份修正不能把沙箱用户带回宿主。
    #[test]
    fn non_admin_console_env_is_sandbox() {
        let cfg = sandbox_deployment_cfg();
        assert_eq!(
            console_env_plan(&cfg, "conv-alice-console", "alice"),
            ConsoleEnvPlan::Sandbox
        );
        // 真建容器由 `tests/desktop_tier_it.rs` 覆盖，这里只锁走向。
    }

    #[cfg(unix)]
    #[test]
    fn workspace_supports_independent_console_sessions() {
        let root = tempfile::tempdir().expect("workspace");
        let manager = ConsoleSessionManager::default();
        let first = manager
            .open_session(
                host_env(root.path()),
                root.path().to_str().expect("utf8 path"),
                "test-conversation",
                None,
                100,
                40,
            )
            .expect("create first shell");
        let second = manager
            .open_session(
                host_env(root.path()),
                root.path().to_str().expect("utf8 path"),
                "test-conversation",
                None,
                80,
                24,
            )
            .expect("create second shell");
        assert_ne!(first.id, second.id);
        assert_eq!(first.cwd, second.cwd);
        manager
            .write(&first.id, "echo first\r")
            .expect("write first");
        manager.resize(&second.id, 120, 50).expect("resize second");
        assert!(manager.close(&first.id));
        manager
            .write(&second.id, "echo second\r")
            .expect("second remains alive");
        assert!(manager.close(&second.id));
    }

    /// Verifies the PTY input path accepts UTF-8 CJK: write a Chinese echo and
    /// assert the shell echoes it back. Guards against silent loss in the
    /// `write` → master writer chain (frontend onData → invoke → this writer).
    #[cfg(unix)]
    #[test]
    fn console_session_roundtrips_utf8_input() {
        let dir = tempfile::tempdir().expect("workspace");
        let info = ConsoleSessionInfo {
            id: "utf8-test".into(),
            workspace_root: dir.path().to_str().expect("utf8 path").into(),
            cwd: dir.path().to_str().expect("utf8 path").into(),
            label: "utf8-test".into(),
            conversation_id: "test-conversation".into(),
        };
        let env: Arc<dyn crate::env::Environment> = Arc::new(crate::env::LocalEnv::new(dir.path()));
        let session = spawn_console_session(env, info, 100, 40).expect("spawn shell");
        let mut reader = session.pty.take_reader().expect("take reader");
        let mut sink = String::new();
        // Drain the initial prompt so the assertion below only matches our echo.
        let mut buf = [0u8; 4096];
        for _ in 0..20 {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => sink.push_str(&String::from_utf8_lossy(&buf[..count])),
                Err(_) => break,
            }
            if sink.contains('$') || sink.contains('%') || sink.contains('#') {
                break;
            }
        }
        session
            .pty
            .write("echo 中文往返测试\r".as_bytes())
            .expect("write utf8");
        let mut echoed = String::new();
        for _ in 0..50 {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => echoed.push_str(&String::from_utf8_lossy(&buf[..count])),
                Err(_) => break,
            }
            if echoed.contains("中文往返测试") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            echoed.contains("中文往返测试"),
            "PTY did not echo CJK input; read back: {echoed:?}"
        );
        let _ = session.pty.kill();
    }

    #[test]
    fn normalize_directory_strips_extended_path_prefix() {
        // Uses the current directory so the test is portable across OSes and
        // CI machines; on Windows canonicalize() returns a `\\?\`-prefixed
        // path, which must be stripped before it is used as a shell cwd.
        let cwd = std::env::current_dir().expect("current dir");
        let input = cwd.to_string_lossy().to_string();
        let normalized = normalize_directory(&input, "测试目录").expect("normalize");
        assert!(
            !normalized.starts_with(r"\\?\"),
            "normalized path must not keep the extended prefix: {normalized}"
        );
        assert!(
            Path::new(&normalized).is_dir(),
            "normalized path must exist"
        );
        assert!(
            Path::new(&normalized).canonicalize().is_ok(),
            "normalized path must be resolvable"
        );
    }
}
