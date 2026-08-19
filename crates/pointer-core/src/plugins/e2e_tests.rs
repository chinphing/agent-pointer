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

    // 极简 MCP server：按 id 回显响应（initialize/tools/list/tools/call）
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
