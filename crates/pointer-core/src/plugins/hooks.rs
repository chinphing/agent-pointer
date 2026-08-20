//! 插件 hooks 执行器（P3）：解析插件目录 `hooks/hooks.json`（Claude 约定），
//! 把 `PreToolUse` / `PostToolUse` 声明注册到 [`crate::dispatcher::HookRegistry`]，
//! 以 sidecar 进程执行 command（stdio + JSON 事件），按输出决策 allow / block。
//!
//! hooks.json 结构（与 Claude Code 兼容的简化形态）：
//! ```json
//! { "hooks": { "PreToolUse": [{ "matcher": "*", "command": "bin/check.sh" }] } }
//! ```
//!
//! 协议：
//! - 输入：stdin 写 `{"hook_event_name":"PreToolUse","tool_name":"…","tool_input":{…}}`；
//! - 输出：stdout 写 `"allow"`、`{"decision":"allow"}` 或 `{"decision":"block","reason":"…"}`。
//! 退出码 0 + 空输出视为 allow；非零退出码 / 超时 / 无法解析 → 默认记日志并放行
//! （fail-open：插件已授权，脚本故障不应卡死用户工作流；block 仅在显式输出时生效）。
//! PreToolUse entry 可配 `"fail_closed": true` 切换为 fail-closed：脚本失败 /
//! 超时 / 无效输出时返回 `Reject`（阻断该工具调用并透出原因）。PostToolUse
//! 是观察者，恒 fail-open（仅记日志）。

use crate::dispatcher::{
    HookIdentity, HookOutcome, HookRegistry, OnRunCancelledHook, OnRunFailedHook,
    OnRunFinishedHook, OnRunStartedHook, PostToolCallContext, PostToolCallHook,
    PreToolCallContext, PreToolCallHook, RunCancelledContext, RunFailedContext,
    RunFinishedContext, RunStartedContext,
};
use crate::chat_service::AppState;
use crate::observability::{SpanKind, TraceEvent};
use crate::plugins::registry::PluginRecord;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// hooks 声明文件名（importer 原样拷贝保留）。
const HOOKS_FILE: &str = "hooks/hooks.json";
/// hook 命令默认超时（毫秒）。
const DEFAULT_HOOK_TIMEOUT_MS: u64 = 30_000;

/// Claude hooks.json 解析模型。
#[derive(Debug, Default, Deserialize)]
struct HooksFile {
    #[serde(default)]
    hooks: HooksDecl,
}

#[derive(Debug, Default, Deserialize)]
#[allow(non_snake_case)] // 字段名 = hooks.json 协议 key（Claude 约定 PreToolUse/PostToolUse/SessionStart/SessionEnd）
struct HooksDecl {
    #[serde(default)]
    PreToolUse: Vec<HookEntry>,
    #[serde(default)]
    PostToolUse: Vec<HookEntry>,
    /// Run 级事件（设计稿 §5 审查注 5.3）：Pointer 中映射为每次 Run 开始/结束，
    /// 与 Claude Code 的"整场会话"语义不同——按 Claude 语义编写的插件迁移后
    /// SessionStart 会每 Run 触发一次。纯观察者，不可阻断，恒 fail-open。
    #[serde(default)]
    SessionStart: Vec<HookEntry>,
    #[serde(default)]
    SessionEnd: Vec<HookEntry>,
}

#[derive(Debug, Deserialize)]
struct HookEntry {
    #[serde(default = "default_matcher")]
    matcher: String,
    command: String,
    /// 失败策略：true = fail-closed（脚本失败/超时/无效输出时阻断）；默认 false = fail-open。
    #[serde(default)]
    fail_closed: bool,
}

fn default_matcher() -> String {
    "*".to_string()
}

/// 注册插件 hooks（激活时调用）。无 `hooks/hooks.json` 时静默跳过；
/// 重复注册按 override_key 替换（幂等）。
pub fn register_plugin_hooks(hook_registry: &HookRegistry, record: &PluginRecord) -> Result<()> {
    let hooks_path = record.dir.join(HOOKS_FILE);
    if !hooks_path.is_file() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&hooks_path)
        .map_err(|e| anyhow!("读取 {} 失败: {e}", hooks_path.display()))?;
    let parsed: HooksFile = serde_json::from_str(&raw)
        .map_err(|e| anyhow!("解析 {} 失败: {e}", hooks_path.display()))?;
    let n_pre = parsed.hooks.PreToolUse.len();
    let n_post = parsed.hooks.PostToolUse.len();
    let n_start = parsed.hooks.SessionStart.len();
    let n_end = parsed.hooks.SessionEnd.len();

    for entry in parsed.hooks.PreToolUse {
        hook_registry.register_pre_tool_call(Arc::new(PluginPreToolCallHook {
            plugin_id: record.id.clone(),
            matcher: entry.matcher.clone(),
            command: entry.command.clone(),
            plugin_dir: record.dir.clone(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
            fail_closed: entry.fail_closed,
        }));
    }
    for entry in parsed.hooks.PostToolUse {
        hook_registry.register_post_tool_call(Arc::new(PluginPostToolCallHook {
            plugin_id: record.id.clone(),
            matcher: entry.matcher.clone(),
            command: entry.command.clone(),
            plugin_dir: record.dir.clone(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        }));
    }
    // Run 级事件（§5 审查注 5.3）：SessionStart → on_run_started；
    // SessionEnd → on_run_finished/failed/cancelled（三槽位各注册一份，
    // 任一终态触发一次）。纯观察者，恒 fail-open。
    for entry in parsed.hooks.SessionStart {
        let hook = Arc::new(PluginRunStartedHook {
            plugin_id: record.id.clone(),
            command: entry.command.clone(),
            plugin_dir: record.dir.clone(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        });
        hook_registry.register_on_run_started(hook);
    }
    for entry in parsed.hooks.SessionEnd {
        let command = entry.command.clone();
        let plugin_dir = record.dir.clone();
        let plugin_id = record.id.clone();
        hook_registry.register_on_run_finished(Arc::new(PluginRunFinishedHook {
            plugin_id: plugin_id.clone(),
            command: command.clone(),
            plugin_dir: plugin_dir.clone(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        }));
        hook_registry.register_on_run_failed(Arc::new(PluginRunFailedHook {
            plugin_id: plugin_id.clone(),
            command: command.clone(),
            plugin_dir: plugin_dir.clone(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        }));
        hook_registry.register_on_run_cancelled(Arc::new(PluginRunCancelledHook {
            plugin_id,
            command,
            plugin_dir,
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        }));
    }
    if n_pre > 0 || n_post > 0 || n_start > 0 || n_end > 0 {
        log::info!(
            "plugin {}: 注册 hooks (PreToolUse={n_pre} PostToolUse={n_post} SessionStart={n_start} SessionEnd={n_end})",
            record.id
        );
    }
    Ok(())
}

/// 注销插件 hooks（禁用 / 卸载时调用），按 `plugin:{id}:hooks:*` 前缀精确移除。
pub fn unregister_plugin_hooks(hook_registry: &HookRegistry, plugin_id: &str) {
    let n_pre = hook_registry.remove_pre_tool_call_by_prefix(&format!("plugin:{plugin_id}:hooks:pre"));
    let n_post =
        hook_registry.remove_post_tool_call_by_prefix(&format!("plugin:{plugin_id}:hooks:post"));
    let n_start = hook_registry.remove_on_run_started_by_prefix(&format!("plugin:{plugin_id}:hooks:start"));
    let n_end = hook_registry
        .remove_on_run_finished_by_prefix(&format!("plugin:{plugin_id}:hooks:end"))
        + hook_registry.remove_on_run_failed_by_prefix(&format!("plugin:{plugin_id}:hooks:end"))
        + hook_registry.remove_on_run_cancelled_by_prefix(&format!("plugin:{plugin_id}:hooks:end"));
    if n_pre + n_post + n_start + n_end > 0 {
        log::info!(
            "plugin {plugin_id}: 注销 hooks (pre={n_pre} post={n_post} start={n_start} end={n_end})"
        );
    }
}

fn matcher_matches(matcher: &str, tool_name: &str) -> bool {
    matcher == "*" || matcher == tool_name
}

/// PreToolUse hook：执行插件命令，按决策返回 Continue / Reject。
/// `fail_closed` 为 true 时，脚本失败 / 超时 / 无效输出返回 Reject（阻断）。
struct PluginPreToolCallHook {
    plugin_id: String,
    matcher: String,
    command: String,
    plugin_dir: PathBuf,
    timeout_ms: u64,
    fail_closed: bool,
}

#[async_trait]
impl PreToolCallHook for PluginPreToolCallHook {
    async fn execute(&self, ctx: &PreToolCallContext<'_>) -> Result<HookOutcome> {
        if !matcher_matches(&self.matcher, ctx.tool_name) {
            return Ok(HookOutcome::Continue);
        }
        let event = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": ctx.tool_name,
            "tool_input": ctx.args,
        });
        // P3④：Hook Span（Run 子节点，与 ToolCall span 平级；只存元数据不存原文）。
        let mut span = TraceEvent::new(
            ctx.run_id.to_string(),
            uuid::Uuid::new_v4().to_string(),
            SpanKind::Hook,
            format!("hook:{}:{}", self.plugin_id, ctx.tool_name),
        );
        span.parent_span_id = Some("run-root".to_string());
        span.run_id = ctx.run_id.to_string();
        span.conversation_id = ctx.conversation_id.to_string();
        if let serde_json::Value::Object(ref mut attrs) = span.attributes {
            attrs.insert("plugin_id".into(), serde_json::json!(self.plugin_id));
            attrs.insert("matcher".into(), serde_json::json!(self.matcher));
            attrs.insert("command".into(), serde_json::json!(self.command));
            attrs.insert("tool_name".into(), serde_json::json!(ctx.tool_name));
        }

        let outcome =
            match run_hook_command(&self.plugin_dir, &self.command, &event, self.timeout_ms).await
            {
                Ok(HookDecision::Allow) => HookOutcome::Continue,
                Ok(HookDecision::Block(reason)) => {
                    log::info!(
                        "plugin {} PreToolUse hook blocked {}: {}",
                        self.plugin_id,
                        ctx.tool_name,
                        reason
                    );
                    if let serde_json::Value::Object(ref mut attrs) = span.attributes {
                        attrs.insert("decision".into(), serde_json::json!("block"));
                        attrs.insert("reason".into(), serde_json::json!(reason));
                    }
                    HookOutcome::Reject { reason }
                }
                Err(e) => {
                    if self.fail_closed {
                        let reason = format!("插件 hook 执行失败（fail-closed 阻断）: {e:#}");
                        log::warn!(
                            "plugin {} PreToolUse hook 执行失败，阻断: {e:#}",
                            self.plugin_id
                        );
                        span.set_error("hook_failed", format!("{e:#}"));
                        HookOutcome::Reject { reason }
                    } else {
                        log::warn!(
                            "plugin {} PreToolUse hook 执行失败，放行: {e:#}",
                            self.plugin_id
                        );
                        span.set_error("hook_failed", format!("{e:#}"));
                        HookOutcome::Continue
                    }
                }
            };
        span.end();
        ctx.state.trace_bus.emit(span);
        Ok(outcome)
    }
}

impl HookIdentity for PluginPreToolCallHook {
    fn override_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("plugin:{}:hooks:pre:{}", self.plugin_id, self.matcher))
    }
    fn sort_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("_70_plugin_hooks:pre:{}", self.plugin_id))
    }
}

/// PostToolUse hook：执行插件命令，失败仅记日志（观察者不阻断）。
struct PluginPostToolCallHook {
    plugin_id: String,
    matcher: String,
    command: String,
    plugin_dir: PathBuf,
    timeout_ms: u64,
}

#[async_trait]
impl PostToolCallHook for PluginPostToolCallHook {
    async fn execute(&self, ctx: &PostToolCallContext<'_>) -> Result<()> {
        if !matcher_matches(&self.matcher, ctx.tool_name) {
            return Ok(());
        }
        let event = serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": ctx.tool_name,
            "tool_response": {
                "status": ctx.status,
                "result": ctx.result,
                "error": ctx.error,
            },
        });
        // P3④：Hook Span（观察者，仅状态；失败记 error）。
        let mut span = TraceEvent::new(
            ctx.run_id.to_string(),
            uuid::Uuid::new_v4().to_string(),
            SpanKind::Hook,
            format!("hook:{}:{}", self.plugin_id, ctx.tool_name),
        );
        span.parent_span_id = Some("run-root".to_string());
        span.run_id = ctx.run_id.to_string();
        span.conversation_id = ctx.conversation_id.to_string();
        if let serde_json::Value::Object(ref mut attrs) = span.attributes {
            attrs.insert("plugin_id".into(), serde_json::json!(self.plugin_id));
            attrs.insert("matcher".into(), serde_json::json!(self.matcher));
            attrs.insert("command".into(), serde_json::json!(self.command));
            attrs.insert("tool_name".into(), serde_json::json!(ctx.tool_name));
        }
        if let Err(e) =
            run_hook_command(&self.plugin_dir, &self.command, &event, self.timeout_ms).await
        {
            log::warn!(
                "plugin {} PostToolUse hook 执行失败（忽略）: {e:#}",
                self.plugin_id
            );
            span.set_error("hook_failed", format!("{e:#}"));
        }
        span.end();
        ctx.state.trace_bus.emit(span);
        Ok(())
    }
}

impl HookIdentity for PluginPostToolCallHook {
    fn override_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("plugin:{}:hooks:post:{}", self.plugin_id, self.matcher))
    }
    fn sort_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("_70_plugin_hooks:post:{}", self.plugin_id))
    }
}

/// SessionStart hook（Run 级）：每次 Run 开始时执行插件命令。
/// 纯观察者——输出/退出码不影响 Run，失败仅记日志（恒 fail-open）。
struct PluginRunStartedHook {
    plugin_id: String,
    command: String,
    plugin_dir: PathBuf,
    timeout_ms: u64,
}

#[async_trait]
impl OnRunStartedHook for PluginRunStartedHook {
    async fn execute(&self, ctx: &RunStartedContext) -> Result<()> {
        let event = serde_json::json!({
            "hook_event_name": "SessionStart",
            "run_id": ctx.run_id,
            "conversation_id": ctx.conversation_id,
        });
        let mut span = TraceEvent::new(
            ctx.run_id.clone(),
            uuid::Uuid::new_v4().to_string(),
            SpanKind::Hook,
            format!("hook:{}:SessionStart", self.plugin_id),
        );
        span.parent_span_id = Some("run-root".to_string());
        span.run_id = ctx.run_id.clone();
        span.conversation_id = ctx.conversation_id.clone();
        if let serde_json::Value::Object(ref mut attrs) = span.attributes {
            attrs.insert("plugin_id".into(), serde_json::json!(self.plugin_id));
            attrs.insert("command".into(), serde_json::json!(self.command));
        }
        if let Err(e) =
            run_hook_command(&self.plugin_dir, &self.command, &event, self.timeout_ms).await
        {
            log::warn!(
                "plugin {} SessionStart hook 执行失败（忽略）: {e:#}",
                self.plugin_id
            );
            span.set_error("hook_failed", format!("{e:#}"));
        }
        span.end();
        ctx.state.trace_bus.emit(span);
        Ok(())
    }
}

impl HookIdentity for PluginRunStartedHook {
    fn override_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("plugin:{}:hooks:start", self.plugin_id))
    }
    fn sort_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("_70_plugin_hooks:start:{}", self.plugin_id))
    }
}

/// SessionEnd hook（Run 级，finished 分支）：Run 正常结束时执行。
struct PluginRunFinishedHook {
    plugin_id: String,
    command: String,
    plugin_dir: PathBuf,
    timeout_ms: u64,
}

#[async_trait]
impl OnRunFinishedHook for PluginRunFinishedHook {
    async fn execute(&self, ctx: &RunFinishedContext) -> Result<()> {
        run_session_end_hook(
            &self.plugin_id,
            &self.plugin_dir,
            &self.command,
            self.timeout_ms,
            "finished",
            &ctx.run_id,
            &ctx.conversation_id,
            &ctx.state,
        )
        .await
    }
}

impl HookIdentity for PluginRunFinishedHook {
    fn override_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("plugin:{}:hooks:end", self.plugin_id))
    }
    fn sort_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("_70_plugin_hooks:end:{}", self.plugin_id))
    }
}

/// SessionEnd hook（Run 级，failed 分支）：Run 失败时执行（带 error）。
struct PluginRunFailedHook {
    plugin_id: String,
    command: String,
    plugin_dir: PathBuf,
    timeout_ms: u64,
}

#[async_trait]
impl OnRunFailedHook for PluginRunFailedHook {
    async fn execute(&self, ctx: &RunFailedContext) -> Result<()> {
        run_session_end_hook(
            &self.plugin_id,
            &self.plugin_dir,
            &self.command,
            self.timeout_ms,
            "failed",
            &ctx.run_id,
            &ctx.conversation_id,
            &ctx.state,
        )
        .await
    }
}

impl HookIdentity for PluginRunFailedHook {
    fn override_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("plugin:{}:hooks:end", self.plugin_id))
    }
    fn sort_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("_70_plugin_hooks:end:{}", self.plugin_id))
    }
}

/// SessionEnd hook（Run 级，cancelled 分支）：Run 被取消时执行。
struct PluginRunCancelledHook {
    plugin_id: String,
    command: String,
    plugin_dir: PathBuf,
    timeout_ms: u64,
}

#[async_trait]
impl OnRunCancelledHook for PluginRunCancelledHook {
    async fn execute(&self, ctx: &RunCancelledContext) -> Result<()> {
        run_session_end_hook(
            &self.plugin_id,
            &self.plugin_dir,
            &self.command,
            self.timeout_ms,
            "cancelled",
            &ctx.run_id,
            &ctx.conversation_id,
            &ctx.state,
        )
        .await
    }
}

impl HookIdentity for PluginRunCancelledHook {
    fn override_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("plugin:{}:hooks:end", self.plugin_id))
    }
    fn sort_key(&self) -> Cow<'static, str> {
        Cow::Owned(format!("_70_plugin_hooks:end:{}", self.plugin_id))
    }
}

/// SessionEnd 三分支共用：执行命令 + Hook Span（status 区分终态）。
async fn run_session_end_hook(
    plugin_id: &str,
    plugin_dir: &Path,
    command: &str,
    timeout_ms: u64,
    status: &str,
    run_id: &str,
    conversation_id: &str,
    state: &AppState,
) -> Result<()> {
    let event = serde_json::json!({
        "hook_event_name": "SessionEnd",
        "status": status,
        "run_id": run_id,
        "conversation_id": conversation_id,
    });
    let mut span = TraceEvent::new(
        run_id.to_string(),
        uuid::Uuid::new_v4().to_string(),
        SpanKind::Hook,
        format!("hook:{plugin_id}:SessionEnd"),
    );
    span.parent_span_id = Some("run-root".to_string());
    span.run_id = run_id.to_string();
    span.conversation_id = conversation_id.to_string();
    if let serde_json::Value::Object(ref mut attrs) = span.attributes {
        attrs.insert("plugin_id".into(), serde_json::json!(plugin_id));
        attrs.insert("command".into(), serde_json::json!(command));
        attrs.insert("status".into(), serde_json::json!(status));
    }
    if let Err(e) = run_hook_command(plugin_dir, command, &event, timeout_ms).await {
        log::warn!(
            "plugin {plugin_id} SessionEnd hook 执行失败（忽略）: {e:#}"
        );
        span.set_error("hook_failed", format!("{e:#}"));
    }
    span.end();
    state.trace_bus.emit(span);
    Ok(())
}

enum HookDecision {
    Allow,
    Block(String),
}

/// 解析命令路径：绝对路径直接用；相对路径相对插件根目录。
fn resolve_hook_command(plugin_dir: &Path, command: &str) -> Result<PathBuf> {
    let p = Path::new(command);
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    let candidate = plugin_dir.join(p);
    if !candidate.exists() {
        return Err(anyhow!(
            "插件 hook 命令不存在: {}（相对插件根目录 {plugin_dir:?}）",
            command
        ));
    }
    Ok(candidate)
}

/// 异步执行：spawn 进程（kill_on_drop）→ 写 stdin JSON → 读 stdout → 超时 kill。
/// 超时分支显式 `kill` 子进程再返回（避免子进程不退出导致调用永久阻塞，
/// fail-closed 策略也因此能及时生效）。
async fn run_hook_command(
    plugin_dir: &Path,
    command: &str,
    event: &serde_json::Value,
    timeout_ms: u64,
) -> Result<HookDecision> {
    let command_path = resolve_hook_command(plugin_dir, command)?;
    let event = event.clone();
    let mut child = Command::new(&command_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow!("启动插件 hook 失败 ({}): {e}", command_path.display()))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("无法打开插件 hook stdin"))?;
        stdin
            .write_all(event.to_string().as_bytes())
            .await
            .map_err(|e| anyhow!("写入插件 hook stdin 失败: {e}"))?;
        stdin.flush().await.ok();
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("无法打开插件 hook stdout"))?;
    let read = async {
        let mut out = String::new();
        let mut reader = tokio::io::BufReader::new(stdout);
        reader
            .read_to_string(&mut out)
            .await
            .map_err(|e| anyhow!("读取插件 hook stdout 失败: {e}"))?;
        Ok::<String, anyhow::Error>(out)
    };
    let out = match tokio::time::timeout(Duration::from_millis(timeout_ms), read).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            log::warn!("插件 hook 超时 ({timeout_ms}ms): {}", command_path.display());
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(anyhow!("插件 hook 执行超时（>{timeout_ms}ms）"));
        }
    };
    let status = child
        .wait()
        .await
        .map_err(|e| anyhow!("等待插件 hook 失败: {e}"))?;
    parse_hook_decision(&out, status.code().unwrap_or(-1))
}

/// 解析 stdout 决策：`"allow"` / `{"decision":"allow"}` / `{"decision":"block","reason":…}`。
/// 退出码 0 + 空输出 → allow；非零退出码 → Err（调用方 fail-open）。
fn parse_hook_decision(stdout: &str, exit_code: i32) -> Result<HookDecision> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        if exit_code == 0 {
            return Ok(HookDecision::Allow);
        }
        return Err(anyhow!("插件 hook 退出码 {exit_code}，无输出"));
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if v.as_str() == Some("allow") {
            return Ok(HookDecision::Allow);
        }
        if let Some(decision) = v.get("decision").and_then(|d| d.as_str()) {
            return match decision {
                "allow" | "approve" => Ok(HookDecision::Allow),
                "block" | "deny" => {
                    let reason = v
                        .get("reason")
                        .and_then(|r| r.as_str())
                        .unwrap_or("插件 hook 阻止了该工具调用")
                        .to_string();
                    Ok(HookDecision::Block(reason))
                }
                other => Err(anyhow!("插件 hook 返回未知决策: {other}")),
            };
        }
    }
    if exit_code == 0 {
        Ok(HookDecision::Allow)
    } else {
        Err(anyhow!(
            "插件 hook 退出码 {exit_code}，输出无法解析: {trimmed}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::registry::PluginRegistry;
    use std::fs;

    fn write_hooks_plugin(root: &Path, hooks_json: &str) -> PathBuf {
        let dir = root.join("com.example.hooks");
        fs::create_dir_all(dir.join("hooks")).unwrap();
        fs::write(
            dir.join("pointer-plugin.toml"),
            r#"
[plugin]
id = "com.example.hooks"
name = "Hooks Demo"
version = "1.0.0"
api_version = "v1"

[hooks]
path = "hooks/"
"#,
        )
        .unwrap();
        fs::write(dir.join("hooks/hooks.json"), hooks_json).unwrap();
        dir
    }

    fn write_script(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn parse_hook_decision_handles_allow_and_block() {
        assert!(matches!(
            parse_hook_decision("\"allow\"", 0).unwrap(),
            HookDecision::Allow
        ));
        assert!(matches!(
            parse_hook_decision(r#"{"decision":"allow"}"#, 0).unwrap(),
            HookDecision::Allow
        ));
        match parse_hook_decision(r#"{"decision":"block","reason":"denied"}"#, 0).unwrap() {
            HookDecision::Block(reason) => assert_eq!(reason, "denied"),
            _ => panic!("expected block"),
        }
        assert!(matches!(parse_hook_decision("", 0).unwrap(), HookDecision::Allow));
        assert!(parse_hook_decision("", 1).is_err());
        assert!(parse_hook_decision("garbage", 1).is_err());
        assert!(matches!(
            parse_hook_decision("garbage", 0).unwrap(),
            HookDecision::Allow
        ));
    }

    #[tokio::test]
    async fn register_hooks_from_json_executes_block_decision() {
        // 隔离 storage + 插件主目录（防测试污染真实目录）
        let _lock = crate::storage::test_app_data_dir_lock();
        let data_dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(data_dir.path().to_path_buf());
        let plugins_home = tempfile::tempdir().unwrap();
        std::env::set_var("POINTER_HOME", plugins_home.path());

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let dir = write_hooks_plugin(
            root,
            r#"{"hooks":{"PreToolUse":[{"matcher":"*","command":"bin/check.sh"}]}}"#,
        );
        write_script(
            &dir,
            "bin/check.sh",
            "#!/bin/sh\ncat >/dev/null\necho '{\"decision\":\"block\",\"reason\":\"no no\"}'\n",
        );

        let auth_file = tmp.path().join("auth.json");
        let reg = PluginRegistry::with_auth_path(auth_file);
        reg.scan_roots(&[(dir, true)]).unwrap();
        let record = reg.get("com.example.hooks").unwrap();

        let hook_registry = HookRegistry::new();
        register_plugin_hooks(&hook_registry, &record).unwrap();
        let state = crate::chat_service::AppState::new();
        let ctx = PreToolCallContext {
            run_id: "r1",
            conversation_id: "c1",
            message_id: "m1",
            tool_call_id: "t1",
            tool_name: "terminal",
            args: &serde_json::json!({ "command": "ls" }),
            state: &state,
        };
        let outcome = hook_registry.run_pre_tool_call(&ctx).await.unwrap();
        match outcome {
            HookOutcome::Reject { reason } => assert_eq!(reason, "no no"),
            other => panic!("expected reject, got {other:?}"),
        }

        // 注销后不再有插件 pre hooks
        unregister_plugin_hooks(&hook_registry, "com.example.hooks");
        assert_eq!(
            hook_registry.remove_pre_tool_call_by_prefix("plugin:com.example.hooks:hooks:pre"),
            0
        );
    }

    #[tokio::test]
    async fn fail_closed_controls_err_branch() {
        // 隔离 storage + 插件主目录（防测试污染真实目录）
        let _lock = crate::storage::test_app_data_dir_lock();
        let data_dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(data_dir.path().to_path_buf());
        let plugins_home = tempfile::tempdir().unwrap();
        std::env::set_var("POINTER_HOME", plugins_home.path());

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("com.example.hooks");
        fs::create_dir_all(dir.join("bin")).unwrap();
        // 失败脚本：exit 1 且无输出 → run_hook_command 返回 Err
        write_script(&dir, "bin/fail.sh", "#!/bin/sh\nexit 1\n");

        let state = crate::chat_service::AppState::new();
        let ctx = PreToolCallContext {
            run_id: "r1",
            conversation_id: "c1",
            message_id: "m1",
            tool_call_id: "t1",
            tool_name: "terminal",
            args: &serde_json::json!({ "command": "ls" }),
            state: &state,
        };

        // fail-open（默认）：脚本失败 → Continue
        let hook_open = PluginPreToolCallHook {
            plugin_id: "com.example.hooks".into(),
            matcher: "*".into(),
            command: "bin/fail.sh".into(),
            plugin_dir: dir.clone(),
            timeout_ms: 5_000,
            fail_closed: false,
        };
        assert!(matches!(
            hook_open.execute(&ctx).await.unwrap(),
            HookOutcome::Continue
        ));

        // fail-closed：脚本失败 → Reject 且 reason 透出
        let hook_closed = PluginPreToolCallHook {
            plugin_id: "com.example.hooks".into(),
            matcher: "*".into(),
            command: "bin/fail.sh".into(),
            plugin_dir: dir.clone(),
            timeout_ms: 5_000,
            fail_closed: true,
        };
        match hook_closed.execute(&ctx).await.unwrap() {
            HookOutcome::Reject { reason } => assert!(reason.contains("fail-closed")),
            other => panic!("expected reject for fail-closed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hook_timeout_kills_child_and_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("com.example.hooks");
        fs::create_dir_all(dir.join("bin")).unwrap();
        // 卡死脚本：不读 stdin、不退出（若不 kill，waiter.join 会永久阻塞）
        write_script(&dir, "bin/hang.sh", "#!/bin/sh\nsleep 30\n");

        let started = std::time::Instant::now();
        // 300ms 超时；若超时分支不 kill 子进程，此调用会在 sleep 30 结束后才返回（测试超时失败）
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_hook_command(&dir, "bin/hang.sh", &serde_json::json!({}), 300),
        )
        .await;
        assert!(
            result.is_ok(),
            "超时分支应快速返回（子进程已 kill），当前耗时 {:?}",
            started.elapsed()
        );
        assert!(result.unwrap().is_err());
    }

    /// SessionStart/SessionEnd 注册到正确的 Run 级槽位，且可被注销。
    /// 确定性测试（不 spawn 进程）：用 `remove_*_by_prefix` 返回值验证注册/注销。
    #[tokio::test]
    async fn session_hooks_register_to_run_slots_and_unregister() {
        // 隔离 storage + 插件主目录（防测试污染真实目录）
        let _lock = crate::storage::test_app_data_dir_lock();
        let data_dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(data_dir.path().to_path_buf());
        let plugins_home = tempfile::tempdir().unwrap();
        std::env::set_var("POINTER_HOME", plugins_home.path());

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let dir = write_hooks_plugin(
            root,
            r#"{"hooks":{"SessionStart":[{"command":"bin/start.sh"}],"SessionEnd":[{"command":"bin/end.sh"}]}}"#,
        );
        // 命令文件无需可执行（本测试只验证注册/注销，不执行）
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("bin/start.sh"), "placeholder").unwrap();
        fs::write(dir.join("bin/end.sh"), "placeholder").unwrap();

        let auth_file = tmp.path().join("auth.json");
        let reg = PluginRegistry::with_auth_path(auth_file);
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        let record = reg.get("com.example.hooks").unwrap();

        let hook_registry = HookRegistry::new();
        register_plugin_hooks(&hook_registry, &record).unwrap();

        // SessionStart → on_run_started 槽位；SessionEnd → finished/failed/cancelled 三槽位
        assert_eq!(
            hook_registry.remove_on_run_started_by_prefix("plugin:com.example.hooks:hooks:start"),
            1
        );
        assert_eq!(
            hook_registry.remove_on_run_finished_by_prefix("plugin:com.example.hooks:hooks:end"),
            1
        );
        assert_eq!(
            hook_registry.remove_on_run_failed_by_prefix("plugin:com.example.hooks:hooks:end"),
            1
        );
        assert_eq!(
            hook_registry.remove_on_run_cancelled_by_prefix("plugin:com.example.hooks:hooks:end"),
            1
        );

        // 重新注册后，unregister_plugin_hooks 应全部移除
        register_plugin_hooks(&hook_registry, &record).unwrap();
        unregister_plugin_hooks(&hook_registry, "com.example.hooks");
        assert_eq!(
            hook_registry.remove_on_run_started_by_prefix("plugin:com.example.hooks:hooks:start"),
            0
        );
        assert_eq!(
            hook_registry.remove_on_run_finished_by_prefix("plugin:com.example.hooks:hooks:end"),
            0
        );
        assert_eq!(
            hook_registry.remove_on_run_failed_by_prefix("plugin:com.example.hooks:hooks:end"),
            0
        );
        assert_eq!(
            hook_registry.remove_on_run_cancelled_by_prefix("plugin:com.example.hooks:hooks:end"),
            0
        );
    }

    /// SessionStart/SessionEnd hook 真实执行命令（marker 文件证明被调用）。
    /// 命令扩展名按平台选择（Windows .bat / Unix .sh）。
    #[tokio::test]
    async fn session_start_end_hooks_execute_command() {
        // 隔离 storage + 插件主目录（防测试污染真实目录）
        let _lock = crate::storage::test_app_data_dir_lock();
        let data_dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(data_dir.path().to_path_buf());
        let plugins_home = tempfile::tempdir().unwrap();
        std::env::set_var("POINTER_HOME", plugins_home.path());

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        #[cfg(windows)]
        let hooks_json = r#"{"hooks":{"SessionStart":[{"command":"bin/start.bat"}],"SessionEnd":[{"command":"bin/end.bat"}]}}"#;
        #[cfg(not(windows))]
        let hooks_json = r#"{"hooks":{"SessionStart":[{"command":"bin/start.sh"}],"SessionEnd":[{"command":"bin/end.sh"}]}}"#;

        let dir = write_hooks_plugin(root, hooks_json);
        #[cfg(windows)]
        {
            fs::create_dir_all(dir.join("bin")).unwrap();
            fs::write(
                dir.join("bin/start.bat"),
                "@echo off\r\necho ok > \"%~dp0start.marker\"\r\n",
            )
            .unwrap();
            fs::write(
                dir.join("bin/end.bat"),
                "@echo off\r\necho ok > \"%~dp0end.marker\"\r\n",
            )
            .unwrap();
        }
        #[cfg(not(windows))]
        {
            write_script(
                &dir,
                "bin/start.sh",
                "#!/bin/sh\necho ok > \"$(dirname \"$0\")/start.marker\"\n",
            );
            write_script(
                &dir,
                "bin/end.sh",
                "#!/bin/sh\necho ok > \"$(dirname \"$0\")/end.marker\"\n",
            );
        }

        let auth_file = tmp.path().join("auth.json");
        let reg = PluginRegistry::with_auth_path(auth_file);
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        let record = reg.get("com.example.hooks").unwrap();

        let hook_registry = HookRegistry::new();
        register_plugin_hooks(&hook_registry, &record).unwrap();

        let state = Arc::new(crate::chat_service::AppState::new());
        let start_ctx = RunStartedContext {
            run_id: "r1".into(),
            conversation_id: "c1".into(),
            state: state.clone(),
        };
        hook_registry.run_on_run_started(&start_ctx).await;
        assert!(
            dir.join("bin").join("start.marker").exists(),
            "SessionStart hook 应被执行"
        );

        let end_ctx = RunFinishedContext {
            run_id: "r1".into(),
            conversation_id: "c1".into(),
            state: state.clone(),
        };
        hook_registry.run_on_run_finished(&end_ctx).await;
        assert!(
            dir.join("bin").join("end.marker").exists(),
            "SessionEnd hook 应被执行"
        );
    }
}
