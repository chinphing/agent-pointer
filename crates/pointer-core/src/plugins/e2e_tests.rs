//! 插件系统端到端验证（真实链路，lib 内 cfg(test)）：隔离环境 → 造插件 →
//! AppState 生命周期 → 验证 hooks 阻断 / MCP 工具注册调用 / 禁用后全部注销 /
//! 工作区 auth 隔离。
//!
//! 覆盖单测没有接线的路径：`AppState::plugin_enable` 完整装配（tools/skills/
//! hooks/MCP 会话）+ `plugin_disable` 完整注销。

use crate::chat_service::AppState;
use crate::dispatcher::{HookOutcome, PreToolCallContext};
use crate::plugins::registry::{PluginRegistry, PluginStatus};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 隔离 storage 数据目录 + 插件主目录（防测试污染真实 `~/.pointer`）。
struct TestIsolation {
    _lock: std::sync::MutexGuard<'static, ()>,
    _data: tempfile::TempDir,
    plugins_home: tempfile::TempDir,
    workspace: tempfile::TempDir,
}

fn isolate() -> TestIsolation {
    let lock = crate::storage::test_app_data_dir_lock();
    let data = tempfile::tempdir().expect("temp data dir");
    crate::storage::set_test_app_data_dir(data.path().to_path_buf());
    let plugins_home = tempfile::tempdir().expect("temp plugins home");
    std::env::set_var("POINTER_HOME", plugins_home.path());
    let workspace = tempfile::tempdir().expect("temp workspace");
    TestIsolation {
        _lock: lock,
        _data: data,
        plugins_home,
        workspace,
    }
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

/// 造一个"全能力"插件：sidecar 工具 + skill + hooks（PreToolUse 阻断）+ MCP server。
fn write_full_plugin(root: &Path, plugin_id: &str) -> PathBuf {
    let dir = root.join(plugin_id);
    fs::create_dir_all(dir.join("skills/demo-skill")).unwrap();
    fs::create_dir_all(dir.join("bin")).unwrap();
    fs::create_dir_all(dir.join("hooks")).unwrap();

    let manifest = format!(
        r#"[plugin]
id = "{plugin_id}"
name = "E2E 插件"
version = "1.0.0"
api_version = "v1"
description = "端到端验收插件"

[skills]
path = "skills/"

[hooks]
path = "hooks/"

[[mcp_servers.server]]
name = "demo-mcp"
transport = "stdio"
command = "bin/demo-mcp"

[[tools.tool]]
name = "demo_hello"
risk_level = "low"
requires_approval = false
description = "问候演示工具"
exec = {{ command = "bin/demo-tool", transport = "sidecar" }}
"#
    );
    fs::write(dir.join("pointer-plugin.toml"), manifest).unwrap();

    fs::write(
        dir.join("skills/demo-skill/SKILL.md"),
        "---\nname: demo-skill\ndescription: 演示技能\n---\n\n演示内容。\n",
    )
    .unwrap();

    // sidecar 工具：echo hello
    write_script(
        &dir,
        "bin/demo-tool",
        "#!/bin/sh\ncat >/dev/null\necho '{\"ok\":true,\"result\":\"hello\"}'\n",
    );

    // hooks：任何 PreToolUse 都 block（验证阻断 + reason 透出）
    fs::write(
        dir.join("hooks/hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"*","command":"bin/check.sh"}]}}"#,
    )
    .unwrap();
    write_script(
        &dir,
        "bin/check.sh",
        "#!/bin/sh\ncat >/dev/null\necho '{\"decision\":\"block\",\"reason\":\"e2e block\"}'\n",
    );

    // 极简 MCP server：按 id 回显响应（initialize/tools/list/tools/call）；
    // tools/call 参数含 "crash" 时直接退出（模拟进程崩溃，供 watchdog 测试）。
    write_script(
        &dir,
        "bin/demo-mcp",
        r#"#!/bin/bash
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  if [ -z "$id" ]; then continue; fi
  if printf '%s' "$line" | grep -q '"initialize"'; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"demo-mcp\",\"version\":\"1.0\"}}}"
  elif printf '%s' "$line" | grep -q '"tools/list"'; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"mcp_echo\",\"description\":\"echo via mcp\",\"inputSchema\":{\"type\":\"object\"}}]}}"
  elif printf '%s' "$line" | grep -q '"tools/call"'; then
    if printf '%s' "$line" | grep -q '"crash"'; then
      exit 0
    fi
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"mcp-e2e-ok\"}],\"isError\":false}}"
  fi
done
"#,
    );

    dir
}

#[tokio::test]
async fn full_plugin_lifecycle_hooks_mcp_and_teardown() {
    let iso = isolate();
    let plugins_root = iso.plugins_home.path().join("plugins");
    fs::create_dir_all(&plugins_root).unwrap();
    let dir = write_full_plugin(&plugins_root, "com.example.e2e");

    // AppState 启动即扫描 POINTER_HOME 插件
    let state = Arc::new(AppState::new());
    let rec = state.plugins.get("com.example.e2e").expect("scanned");
    assert_eq!(rec.status, PluginStatus::Discovered);
    assert_eq!(rec.dir, dir);

    // ---- 启用：sidecar 工具 + skill + hooks + MCP 全部装配 ----
    state.plugin_enable("com.example.e2e").unwrap();
    assert!(state.tools.get_def("demo_hello").is_some(), "sidecar tool");
    assert!(
        state.skills.get("demo-skill").is_some(),
        "plugin skill registered"
    );
    assert!(
        state.tools.get_def("mcp_echo").is_some(),
        "MCP tool registered via tools/list"
    );
    assert!(
        state.mcp_sessions.has_session("com.example.e2e"),
        "MCP session alive"
    );

    // sidecar 工具可调用
    let out = state
        .tools
        .invoke("demo_hello", serde_json::json!({}))
        .expect("sidecar invoke");
    assert_eq!(out, "hello");

    // MCP 工具可调用（真实 JSON-RPC 往返）
    let mcp_out = state
        .tools
        .invoke("mcp_echo", serde_json::json!({}))
        .expect("mcp invoke");
    assert_eq!(mcp_out, "mcp-e2e-ok");

    // hooks 已注册：PreToolUse 触发 → 阻断 + reason 透出
    let ctx = PreToolCallContext {
        run_id: "e2e-run",
        conversation_id: "e2e-conv",
        message_id: "m1",
        tool_call_id: "t1",
        tool_name: "terminal",
        args: &serde_json::json!({ "command": "ls" }),
        state: &state,
    };
    let outcome = state.hooks.run_pre_tool_call(&ctx).await.unwrap();
    match outcome {
        HookOutcome::Reject { reason } => assert_eq!(reason, "e2e block"),
        other => panic!("expected hook reject, got {other:?}"),
    }

    // ---- 禁用：全部注销 + MCP 会话关闭 ----
    state.plugin_disable("com.example.e2e").unwrap();
    assert!(state.tools.get_def("demo_hello").is_none(), "tool removed");
    assert!(state.tools.get_def("mcp_echo").is_none(), "mcp tool removed");
    assert!(state.skills.get("demo-skill").is_none(), "skill removed");
    assert!(
        !state.mcp_sessions.has_session("com.example.e2e"),
        "mcp session closed"
    );
    // hooks 已注销：再跑 PreToolUse 不触发（Continue，无插件 hook）
    let ctx2 = PreToolCallContext {
        run_id: "e2e-run",
        conversation_id: "e2e-conv",
        message_id: "m2",
        tool_call_id: "t2",
        tool_name: "terminal",
        args: &serde_json::json!({ "command": "ls" }),
        state: &state,
    };
    let outcome2 = state.hooks.run_pre_tool_call(&ctx2).await.unwrap();
    assert!(matches!(outcome2, HookOutcome::Continue));
}

/// 收集 TraceEvent 的测试 exporter（McpRequest/Hook Span 断言用）。
struct CollectingExporter {
    events: std::sync::Mutex<Vec<crate::observability::trace::TraceEvent>>,
}

impl CollectingExporter {
    fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl crate::observability::exporters::TraceExporter for CollectingExporter {
    fn name(&self) -> &str {
        "collecting"
    }
    async fn export(&self, batch: Vec<crate::observability::trace::TraceEvent>) -> Result<(), String> {
        self.events.lock().unwrap().extend(batch);
        Ok(())
    }
}

#[tokio::test]
async fn mcp_tool_meta_and_mcp_request_span() {
    use crate::observability::trace::SpanKind;

    let iso = isolate();
    let plugins_root = iso.plugins_home.path().join("plugins");
    fs::create_dir_all(&plugins_root).unwrap();
    let dir = write_full_plugin(&plugins_root, "com.example.e2e");

    // 收集 bus 替换默认（AppState.trace_bus 为 pub 字段）。
    let collecting = Arc::new(CollectingExporter::new());
    let mut reg = crate::observability::exporters::ExporterRegistry::new();
    reg.register(collecting.clone());
    let bus = crate::observability::start(Arc::new(reg));

    let mut state = Arc::new(AppState::new());
    // AppState::new 不 clone 自身（watchdog 只捕获 tools/plugins/sessions），
    // 此处是唯一强引用，可安全替换 trace_bus 为收集 bus。
    Arc::get_mut(&mut state).unwrap().trace_bus = Arc::new(bus);
    state.plugin_enable("com.example.e2e").unwrap();
    assert!(state.tools.get_def("mcp_echo").is_some());

    // doc_source 解析：`plugin:{id}:mcp:{server}:{tool}`
    let meta = state.tools.mcp_tool_meta("mcp_echo").expect("meta");
    assert_eq!(meta.plugin_id, "com.example.e2e");
    assert_eq!(meta.server, "demo-mcp");
    assert_eq!(meta.tool, "mcp_echo");
    // 非 MCP 工具（sidecar）返回 None
    assert!(state.tools.mcp_tool_meta("demo_hello").is_none());

    // 与 dispatch 相同的 span 发射形态：成功调用
    crate::observability::pipeline::emit_mcp_request_span(
        &state.trace_bus,
        "conv-1",
        "mcp_echo",
        &meta.plugin_id,
        &meta.server,
        &meta.tool,
        "run-1",
        Some("tool-span-1"),
        true,
        3,
    );
    // 失败调用
    crate::observability::pipeline::emit_mcp_request_span(
        &state.trace_bus,
        "conv-1",
        "mcp_echo",
        &meta.plugin_id,
        &meta.server,
        &meta.tool,
        "run-1",
        Some("tool-span-2"),
        false,
        9,
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let n = collecting.events.lock().unwrap().len();
        if n >= 2 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "span 未在预期时间内送达 exporter"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let events = collecting.events.lock().unwrap().clone();
    let mcp: Vec<_> = events
        .iter()
        .filter(|e| e.kind == SpanKind::McpRequest)
        .collect();
    assert_eq!(mcp.len(), 2);
    // 父 span = ToolCall span id（非 run-root）
    assert_eq!(mcp[0].parent_span_id.as_deref(), Some("tool-span-1"));
    assert_eq!(mcp[1].parent_span_id.as_deref(), Some("tool-span-2"));
    assert_eq!(mcp[0].run_id, "run-1");
    assert_eq!(mcp[0].conversation_id, "conv-1");
    assert_eq!(
        mcp[0].attributes.get("server").and_then(|v| v.as_str()),
        Some("demo-mcp")
    );
    assert_eq!(
        mcp[0].attributes.get("plugin_id").and_then(|v| v.as_str()),
        Some("com.example.e2e")
    );
    // 成功 span 无 error；失败 span 有 error
    assert!(mcp[0].error.is_none());
    assert!(mcp[1].error.is_some());
}

#[tokio::test]
async fn hook_execution_emits_hook_span() {
    use crate::observability::trace::SpanKind;

    let iso = isolate();
    let plugins_root = iso.plugins_home.path().join("plugins");
    fs::create_dir_all(&plugins_root).unwrap();
    let dir = write_full_plugin(&plugins_root, "com.example.e2e");

    let collecting = Arc::new(CollectingExporter::new());
    let mut reg = crate::observability::exporters::ExporterRegistry::new();
    reg.register(collecting.clone());
    let bus = crate::observability::start(Arc::new(reg));

    let mut state = Arc::new(AppState::new());
    Arc::get_mut(&mut state).unwrap().trace_bus = Arc::new(bus);
    state.plugin_enable("com.example.e2e").unwrap();

    // 触发 PreToolUse hook（block 脚本）→ Reject + Hook Span
    let ctx = PreToolCallContext {
        run_id: "run-hook",
        conversation_id: "conv-hook",
        message_id: "m1",
        tool_call_id: "t1",
        tool_name: "terminal",
        args: &serde_json::json!({ "command": "ls" }),
        state: &state,
    };
    let outcome = state.hooks.run_pre_tool_call(&ctx).await.unwrap();
    assert!(matches!(outcome, HookOutcome::Reject { .. }));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let has_hook = collecting
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.kind == SpanKind::Hook);
        if has_hook {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Hook span 未在预期时间内送达 exporter"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let events = collecting.events.lock().unwrap().clone();
    let hook: Vec<_> = events
        .iter()
        .filter(|e| e.kind == SpanKind::Hook)
        .collect();
    assert_eq!(hook.len(), 1);
    // parent = run-root（与 ToolCall span 平级，计划 §7.1）
    assert_eq!(hook[0].parent_span_id.as_deref(), Some("run-root"));
    assert_eq!(hook[0].run_id, "run-hook");
    assert_eq!(hook[0].conversation_id, "conv-hook");
    assert_eq!(
        hook[0].attributes.get("plugin_id").and_then(|v| v.as_str()),
        Some("com.example.e2e")
    );
    assert_eq!(
        hook[0].attributes.get("tool_name").and_then(|v| v.as_str()),
        Some("terminal")
    );
    // block 决策 + reason 透出到 span
    assert_eq!(
        hook[0].attributes.get("decision").and_then(|v| v.as_str()),
        Some("block")
    );
    assert_eq!(
        hook[0].attributes.get("reason").and_then(|v| v.as_str()),
        Some("e2e block")
    );
    assert!(hook[0].error.is_none());
}

#[tokio::test]
async fn global_mcp_assembles_and_tools_callable() {
    let iso = isolate();
    let cfg_dir = iso.workspace.path().to_path_buf();
    // 全局 MCP server 脚本（与插件测试同款：按 id 回显）
    write_script(
        &cfg_dir,
        "bin/demo-mcp",
        r#"#!/bin/bash
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  if [ -z "$id" ]; then continue; fi
  if printf '%s' "$line" | grep -q '"initialize"'; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"demo-mcp\",\"version\":\"1.0\"}}}"
  elif printf '%s' "$line" | grep -q '"tools/list"'; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"mcp_echo\",\"description\":\"echo via mcp\",\"inputSchema\":{\"type\":\"object\"}}]}}"
  elif printf '%s' "$line" | grep -q '"tools/call"'; then
    if printf '%s' "$line" | grep -q '"crash"'; then
      exit 0
    fi
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"mcp-e2e-ok\"}],\"isError\":false}}"
  fi
done
"#,
    );

    let decl = crate::plugins::manifest::McpServerDecl {
        name: "demo".into(),
        transport: "stdio".into(),
        command: "bin/demo-mcp".into(),
        args: vec![],
        env: std::collections::HashMap::new(),
    };
    let state = Arc::new(AppState::new());
    state
        .reload_global_mcp(vec![decl], cfg_dir.clone())
        .unwrap();

    // 全局工具命名 `mcp.<server>.<tool>`；可调用
    assert!(state.tools.get_def("mcp.demo.mcp_echo").is_some());
    assert_eq!(
        state
            .tools
            .invoke("mcp.demo.mcp_echo", serde_json::json!({}))
            .unwrap(),
        "mcp-e2e-ok"
    );
    assert!(state.mcp_sessions.has_session(crate::plugins::mcp::GLOBAL_MCP_KEY));

    // 热重载：清空配置 → 工具注销、会话关闭
    state.reload_global_mcp(vec![], cfg_dir.clone()).unwrap();
    assert!(state.tools.get_def("mcp.demo.mcp_echo").is_none());
    assert!(
        !state
            .mcp_sessions
            .has_session(crate::plugins::mcp::GLOBAL_MCP_KEY)
    );
}

#[tokio::test]
async fn global_mcp_conflict_rejects_plugin_enable() {
    let iso = isolate();
    // 先写插件（AppState::new 启动扫描时需要能看到）
    let plugins_root = iso.plugins_home.path().join("plugins");
    fs::create_dir_all(&plugins_root).unwrap();
    let dir = write_full_plugin(&plugins_root, "com.example.e2e");
    let _ = dir;

    let cfg_dir = iso.workspace.path().to_path_buf();
    write_script(
        &cfg_dir,
        "bin/demo-mcp",
        "#!/bin/bash\nwhile IFS= read -r line; do\n  echo '{}'\ndone\n",
    );
    // 全局配置同名 server `demo-mcp`（与插件 full 插件里一致 → 应触发冲突）
    let decl = crate::plugins::manifest::McpServerDecl {
        name: "demo-mcp".into(),
        transport: "stdio".into(),
        command: "bin/demo-mcp".into(),
        args: vec![],
        env: std::collections::HashMap::new(),
    };
    let state = Arc::new(AppState::new());
    state.reload_global_mcp(vec![decl], cfg_dir.clone()).unwrap();

    // 同名冲突：插件启用应被拒绝（全局优先）
    let err = state.plugin_enable("com.example.e2e").unwrap_err();
    assert!(err.to_string().contains("全局优先"), "err: {err}");
}

#[tokio::test]
async fn global_mcp_crash_recovers_via_watchdog() {
    let iso = isolate();
    let cfg_dir = iso.workspace.path().to_path_buf();
    // 全局 MCP server 脚本（含 crash 支持）
    write_script(
        &cfg_dir,
        "bin/demo-mcp",
        r#"#!/bin/bash
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  if [ -z "$id" ]; then continue; fi
  if printf '%s' "$line" | grep -q '"initialize"'; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"demo-mcp\",\"version\":\"1.0\"}}}"
  elif printf '%s' "$line" | grep -q '"tools/list"'; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"mcp_echo\",\"description\":\"echo via mcp\",\"inputSchema\":{\"type\":\"object\"}}]}}"
  elif printf '%s' "$line" | grep -q '"tools/call"'; then
    if printf '%s' "$line" | grep -q '"crash"'; then
      exit 0
    fi
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"mcp-e2e-ok\"}],\"isError\":false}}"
  fi
done
"#,
    );
    let decl = crate::plugins::manifest::McpServerDecl {
        name: "demo".into(),
        transport: "stdio".into(),
        command: "bin/demo-mcp".into(),
        args: vec![],
        env: std::collections::HashMap::new(),
    };
    // AppState::new 已 spawn watchdog（2s 周期）
    let state = Arc::new(AppState::new());
    state.reload_global_mcp(vec![decl], cfg_dir.clone()).unwrap();
    assert_eq!(
        state
            .tools
            .invoke("mcp.demo.mcp_echo", serde_json::json!({}))
            .unwrap(),
        "mcp-e2e-ok"
    );

    // 触发崩溃：脚本退出 → 等 watchdog 自动重建（不手动驱动 cycle）
    let _ = state
        .tools
        .invoke("mcp.demo.mcp_echo", serde_json::json!({ "crash": true }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut recovered = false;
    while std::time::Instant::now() < deadline {
        if state.mcp_sessions.any_alive(crate::plugins::mcp::GLOBAL_MCP_KEY) {
            if let Ok(out) = state
                .tools
                .invoke("mcp.demo.mcp_echo", serde_json::json!({}))
            {
                if out == "mcp-e2e-ok" {
                    recovered = true;
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(
        recovered,
        "watchdog 未在预期时间内自动恢复全局 MCP（2s 周期 + 退避）"
    );
}

#[tokio::test]
async fn mcp_crash_recovers_via_watchdog() {
    let iso = isolate();
    let plugins_root = iso.plugins_home.path().join("plugins");
    fs::create_dir_all(&plugins_root).unwrap();
    let dir = write_full_plugin(&plugins_root, "com.example.e2e");

    let state = Arc::new(AppState::new());
    state.plugin_enable("com.example.e2e").unwrap();
    assert!(state.tools.get_def("mcp_echo").is_some());
    assert_eq!(
        state
            .tools
            .invoke("mcp_echo", serde_json::json!({}))
            .unwrap(),
        "mcp-e2e-ok"
    );

    // 触发崩溃：脚本收到含 "crash" 的调用后退出（stdout EOF → alive=false）。
    let _ = state
        .tools
        .invoke("mcp_echo", serde_json::json!({ "crash": true }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while state.mcp_sessions.any_alive("com.example.e2e") {
        assert!(
            std::time::Instant::now() < deadline,
            "MCP server 未在预期时间内标记崩溃"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // watchdog 一轮：重建会话 + 重新注册工具（sidecar 工具保留）。
    crate::chat_service::app_state::mcp_watchdog_cycle(
        &state.tools,
        &state.plugins,
        &state.mcp_sessions,
        &state.global_mcp,
    );
    assert!(
        state.mcp_sessions.any_alive("com.example.e2e"),
        "watchdog 应重启 MCP server"
    );
    assert!(
        state.tools.get_def("mcp_echo").is_some(),
        "MCP 工具应重新注册"
    );
    assert!(
        state.tools.get_def("demo_hello").is_some(),
        "sidecar 工具应保留（未误注销）"
    );
    assert_eq!(
        state
            .tools
            .invoke("mcp_echo", serde_json::json!({}))
            .unwrap(),
        "mcp-e2e-ok"
    );
}

#[tokio::test]
async fn mcp_degraded_stops_auto_restart() {
    let iso = isolate();
    let plugins_root = iso.plugins_home.path().join("plugins");
    fs::create_dir_all(&plugins_root).unwrap();
    let dir = write_full_plugin(&plugins_root, "com.example.e2e");

    let state = Arc::new(AppState::new());
    state.plugin_enable("com.example.e2e").unwrap();
    assert!(state.mcp_sessions.has_session("com.example.e2e"));

    // 连续失败达上限 → degraded（模拟 watchdog 多次重启失败）。
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    for _ in 0..crate::plugins::mcp::MAX_MCP_RESTART_FAILURES {
        state
            .mcp_sessions
            .mark_failure("com.example.e2e", "boom", now);
    }
    assert!(state.mcp_sessions.is_degraded("com.example.e2e"));

    // 先让会话崩溃，watchdog 轮应因 degraded 跳过（不重建）。
    let _ = state
        .tools
        .invoke("mcp_echo", serde_json::json!({ "crash": true }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while state.mcp_sessions.any_alive("com.example.e2e") {
        assert!(
            std::time::Instant::now() < deadline,
            "MCP server 未在预期时间内标记崩溃"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    crate::chat_service::app_state::mcp_watchdog_cycle(
        &state.tools,
        &state.plugins,
        &state.mcp_sessions,
        &state.global_mcp,
    );
    assert!(
        !state.mcp_sessions.any_alive("com.example.e2e"),
        "degraded 后不应自动重启"
    );
    assert!(
        state.mcp_sessions.is_degraded("com.example.e2e"),
        "degraded 状态应保持"
    );
}

#[test]
fn workspace_level_plugin_auth_does_not_leak_to_user_level() {
    let iso = isolate();
    // 用户级：$POINTER_HOME/plugins/<id>
    let user_root = iso.plugins_home.path().join("plugins");
    fs::create_dir_all(&user_root).unwrap();
    let user_dir = write_full_plugin(&user_root, "com.example.same");
    // 工作区级：<workspace>/.pointer/plugins/<id>（同名）
    let ws_root = iso.workspace.path().join(".pointer/plugins");
    fs::create_dir_all(&ws_root).unwrap();
    let ws_dir = write_full_plugin(&ws_root, "com.example.same");

    let reg = PluginRegistry::with_auth_path(iso.plugins_home.path().join("auth.json"));
    // 用户级插件：授权 + 启用
    reg.scan_roots(&[(user_dir.clone(), true)]).unwrap();
    reg.authorize("com.example.same").unwrap();
    reg.enable("com.example.same").unwrap();

    // 工作区同名插件：未授权（用户级授权不串扰）
    reg.scan_roots(&[(ws_dir.clone(), false)]).unwrap();
    let rec = reg.get("com.example.same").unwrap();
    assert_eq!(rec.dir, ws_dir);
    assert_eq!(rec.status, PluginStatus::Discovered);
    assert!(!rec.authorized);

    // 授权后：auth store 同时存在两条记录（dir 隔离）
    reg.authorize("com.example.same").unwrap();
    let auth = reg.load_auth();
    assert_eq!(auth.plugins.len(), 2);
    assert!(auth.plugins.contains_key(&user_dir.to_string_lossy().to_string()));
    assert!(auth.plugins.contains_key(&ws_dir.to_string_lossy().to_string()));
}
