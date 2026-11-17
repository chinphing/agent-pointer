//! 端到端冒烟：走**真实** `ToolRegistry` + 真实 handler 包装 + thread-local 工作区 guard。
//!
//! 为什么需要它：`tools/file/mod.rs` 里的单测直接调 `execute_file_*`，绕过了注册表、
//! handler 闭包和 `resolve_tool_workspace_root()` 的 guard 重放。这三层任一断掉，工具会
//! 静默失效而单测全绿（例如 env 抽象接入后，handler 拿不到工作区）。
//!
//! 覆盖：`file_write` → `file_read` → `file_edit` → `file_list` → `file_glob` → `file_grep`
//! → `terminal`（git init/add/commit/status/diff）。
//! `read_lints` 会真的调 linter，见文件末尾的 opt-in 用例。

use pointer_core::session_work_dir_env::SessionWorkDirGuard;
use pointer_core::tools::file::{self, AgentWorkspaceGuard};
use pointer_core::tools::terminal;
use pointer_core::tools::ToolRegistry;
use serde_json::{json, Value};

fn registry() -> ToolRegistry {
    let reg = ToolRegistry::new();
    file::register_all(&reg);
    terminal::register_all(&reg);
    reg
}

fn parse(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|e| panic!("工具输出不是 JSON: {e}\n{raw}"))
}

#[test]
fn file_and_terminal_tools_work_through_the_registry() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().display().to_string();
    // 真实分发链：会话工作区 + WORKING_DIR 两个 thread-local 都绑到临时目录。
    let _workspace = AgentWorkspaceGuard::enter(&ws);
    let _work_dir = SessionWorkDirGuard::enter(ws.clone());
    // env 抽象接入后，工具调用需要显式的 ToolContext（会话 + 环境 + 工作区）。
    let env: std::sync::Arc<dyn pointer_core::env::Environment> =
        std::sync::Arc::new(pointer_core::env::LocalEnv::new(ws.clone()));
    let ctx = pointer_core::tools::context::ToolContext::new("smoke", "tester", env, ws.clone());
    let _ctx_guard = pointer_core::tools::context::ToolContextGuard::enter(ctx.clone());
    let reg = registry();

    // 1. file_write（新建 + 自动建父目录）
    let out = parse(
        &reg.invoke(&ctx, 
            "file_write",
            json!({"path": "smoke/a.txt", "content": "hello\n"}),
        )
        .expect("file_write"),
    );
    assert_eq!(out["success"], json!(true), "{out}");
    assert_eq!(out["created"], json!(true), "{out}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("smoke/a.txt")).unwrap(),
        "hello\n"
    );

    // 2. file_read
    let out = parse(&reg.invoke(&ctx, "file_read", json!({"path": "smoke/a.txt"})).expect("file_read"));
    assert_eq!(out["content"], json!("hello"), "{out}");
    assert_eq!(out["totalLines"], json!(1), "{out}");

    // 3. file_edit
    let out = parse(
        &reg.invoke(&ctx, 
            "file_edit",
            json!({"path": "smoke/a.txt", "oldString": "hello", "newString": "hello-world"}),
        )
        .expect("file_edit"),
    );
    assert_eq!(out["success"], json!(true), "{out}");
    assert_eq!(out["replaced"], json!(1), "{out}");
    let out = parse(&reg.invoke(&ctx, "file_read", json!({"path": "smoke/a.txt"})).expect("file_read"));
    assert_eq!(out["content"], json!("hello-world"), "{out}");

    // 4. file_list（递归）：隐藏项与 .git 都要列出来——旧 walkdir 行为，不能被 env.walk 的
    //    默认 hidden / git 过滤悄悄改掉。
    std::fs::write(dir.path().join("smoke/.hidden.txt"), "h").unwrap();
    let out = parse(&reg.invoke(&ctx, "file_list", json!({"path": "smoke"})).expect("file_list"));
    let names: Vec<&str> = out["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .filter_map(|e| e["path"].as_str())
        .collect();
    assert!(
        names.iter().any(|p| p.ends_with("a.txt")),
        "file_list 未返回 a.txt: {names:?}"
    );
    assert!(
        names.iter().any(|p| p.ends_with(".hidden.txt")),
        "file_list 递归漏掉隐藏文件: {names:?}"
    );

    // 5. file_glob
    let out = parse(&reg.invoke(&ctx, "file_glob", json!({"pattern": "**/*.txt"})).expect("file_glob"));
    assert_eq!(out["count"], json!(1), "{out}");

    // 6. file_grep（走一次批量遍历 + 逐文件 open_reader）
    let out = parse(
        &reg.invoke(&ctx, "file_grep", json!({"pattern": "hello-world", "path": "."}))
            .expect("file_grep"),
    );
    assert!(out["count"].as_u64().unwrap_or(0) >= 1, "{out}");
    assert!(
        out["results"][0]["path"]
            .as_str()
            .unwrap_or_default()
            .ends_with("a.txt"),
        "{out}"
    );

    // 7. terminal：git 全链路（用 -c 指定身份，不依赖全局 git 配置）
    let out = parse(
        &reg.invoke(&ctx, 
            "terminal",
            json!({"command": "git init -q . && git add -A && git -c user.name=smoke -c user.email=smoke@local commit -qm smoke && git status --short"}),
        )
        .expect("terminal git init/commit"),
    );
    assert_eq!(out["exitCode"], json!(0), "{out}");
    assert_eq!(
        out["stdout"].as_str().unwrap_or_default().trim(),
        "",
        "刚提交后工作区应为干净: {out}"
    );

    // 8. terminal：git diff 干净树（workspace_read 之外的第二条 git 路径）
    let out = parse(
        &reg.invoke(&ctx, "terminal", json!({"command": "git diff --stat"}))
            .expect("terminal git diff"),
    );
    assert_eq!(out["exitCode"], json!(0), "{out}");

    // 9. 未注册工具要给出可读错误（分发层兜底）
    let err = reg
        .invoke(&ctx, "no_such_tool", json!({}))
        .expect_err("未注册工具应当报错")
        .to_string();
    assert!(err.contains("未注册"), "{err}");
}

/// `read_lints` 会真的调用 linter（cargo clippy），耗时数秒到数十秒，故默认忽略。
/// 手动跑：`cargo test -p pointer-core --test tool_dispatch_smoke -- --ignored`
#[test]
#[ignore = "runs a real linter; opt-in"]
fn read_lints_runs_through_the_registry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"smoke_lint\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "use std::collections::HashMap;\n\npub fn answer() -> i32 {\n    42\n}\n",
    )
    .unwrap();

    let ws = dir.path().display().to_string();
    let _workspace = AgentWorkspaceGuard::enter(&ws);
    let _work_dir = SessionWorkDirGuard::enter(ws.clone());
    // env 抽象接入后，工具调用需要显式的 ToolContext（会话 + 环境 + 工作区）。
    let env: std::sync::Arc<dyn pointer_core::env::Environment> =
        std::sync::Arc::new(pointer_core::env::LocalEnv::new(ws.clone()));
    let ctx = pointer_core::tools::context::ToolContext::new("smoke", "tester", env, ws.clone());
    let _ctx_guard = pointer_core::tools::context::ToolContextGuard::enter(ctx.clone());

    let reg = ToolRegistry::new();
    pointer_core::agents::coder::read_lints::register_all(&reg);

    let raw = reg
        .invoke(&ctx, "read_lints", json!({"paths": ["src/lib.rs"]}))
        .expect("read_lints");
    let out = parse(&raw);
    assert!(
        out["outcome"].is_string() || out["lintExecuted"].is_boolean(),
        "read_lints 输出缺少 outcome/lintExecuted: {out}"
    );
    println!(
        "read_lints(带 paths): outcome={} lintExecuted={} perRun={} diagnostics={}",
        out["outcome"],
        out["lintExecuted"],
        out["runs"][0]["diagnosticCount"],
        out["diagnostics"].as_array().map(|a| a.len()).unwrap_or(0)
    );
    // 回归断言：`src/lib.rs` 里有未使用导入，clippy 必报 warning；
    // 修复前这里会返回 outcome=clean / diagnostics=[]（相对路径被判成工作区外丢弃）。
    assert_eq!(
        out["outcome"], "issues_found",
        "clippy 明明有告警却报成 clean: {out}"
    );
    assert!(
        out["diagnostics"].as_array().map(|a| a.len()).unwrap_or(0) >= 1,
        "diagnostics 不能为空: {out}"
    );

    // 对照组：不带 paths 过滤（同一个回归点，另一种入口）。
    //
    // 历史缺陷（2026-11-15 冒烟发现，已修）：cargo JSON 的 `spans[0].file_name` 是**相对路径**
    // （`src/lib.rs`），`push_filtered_batch` → `path_is_under_workspace` 用当前进程 cwd 去
    // canonicalize，解析不到 workspace 下 → 被判成"工作区外"丢弃 → 告警被误报成 clean。
    // 修法：`parse_cargo_compiler_messages` 增加 cwd 参数，把相对路径拼成绝对路径。
    let raw = reg.invoke(&ctx, "read_lints", json!({})).expect("read_lints(no filter)");
    let unfiltered = parse(&raw);
    println!(
        "read_lints(不过滤): outcome={} perRun={} diagnostics={}",
        unfiltered["outcome"],
        unfiltered["runs"][0]["diagnosticCount"],
        unfiltered["diagnostics"].as_array().map(|a| a.len()).unwrap_or(0)
    );
    assert_eq!(
        unfiltered["outcome"], "issues_found",
        "不带 paths 时也不能报 clean: {unfiltered}"
    );
    assert!(
        unfiltered["outcome"].is_string() || unfiltered["lintExecuted"].is_boolean(),
        "read_lints 输出缺少 outcome/lintExecuted: {unfiltered}"
    );
}
