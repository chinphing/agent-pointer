//! light 档位的会话绑定真机测试（需要 e2b-local 网关）。
//!
//! ```bash
//! cargo test -p pointer-core --test light_binding_it -- --ignored --test-threads=1
//! ```
//!
//! 覆盖 §3.4 / §7.1 B3 的三条主线：
//! 1. 部署默认 light → 会话绑到远端，工作区根变成 guest `/workspace`；
//! 2. admin 账号默认 local → 不建沙箱、工作区根保持宿主路径；
//! 3. 会话回收 → 沙箱销毁、缓存清空。

use std::path::PathBuf;

use pointer_core::env::{current_env, effective_env, session_env, EnvKind, Environment};

fn sandbox_url() -> String {
    std::env::var("POINTER_SANDBOX_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:3000".to_string())
}

#[test]
#[ignore = "需要 e2b-local 网关；手动跑"]
fn light_binding_end_to_end() {
    // 部署默认 light；admin 名单里只有 admin。
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "light");
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_SANDBOX_URL", sandbox_url());
    std::env::set_var("POINTER_SERVER_ADMIN_ACCOUNTS", "admin");
    std::env::remove_var("POINTER_SERVER_ENVIRONMENT_ACCOUNTS");

    // 技能根：默认指向干净的临时目录；外部已给 POINTER_SKILLS_DIR 时尊重它
    // （用来对真实技能库做验证）。真实库里若出现 token.txt，会被挂载前的凭证检查拦下
    // ——那是设计行为，不是测试缺陷。
    let _skills_guard: Option<tempfile::TempDir>;
    if std::env::var("POINTER_SKILLS_DIR")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        let skills = tempfile::tempdir().expect("temp skills dir");
        std::fs::create_dir_all(skills.path().join("demo")).unwrap();
        std::fs::write(skills.path().join("demo/SKILL.md"), "# demo\n").unwrap();
        std::env::set_var("POINTER_SKILLS_DIR", skills.path());
        _skills_guard = Some(skills);
    } else {
        _skills_guard = None;
    }

    // --- 1. 普通用户：绑到远端 ---------------------------------------
    let conv = "conv-light-binding-it";
    let (guard, workspace) = session_env::bind_and_resolve_workspace(
        conv,
        "alice",
        PathBuf::from("/host/should-be-ignored"),
    )
    .expect("bind light session");
    assert!(guard.is_some(), "light 档位必须绑定环境");
    assert_eq!(workspace, "/workspace", "light 下工作区根是 guest 路径");
    assert_eq!(effective_env().kind(), EnvKind::Light);
    assert!(current_env().is_some());

    let sandbox_id = session_env::cached_sandbox_id(conv).expect("沙箱应被缓存");
    eprintln!("sandbox={sandbox_id}");

    // 文件写入真的落在沙箱里
    let env = effective_env();
    let path = std::path::Path::new("/workspace/binding.txt");
    env.write_file(path, b"from-light").expect("write in sandbox");
    assert_eq!(env.read_file(path).expect("read back"), b"from-light");

    // 同一会话再绑一次：复用同一个沙箱
    let (guard2, workspace2) =
        session_env::bind_and_resolve_workspace(conv, "alice", PathBuf::from("/host/x"))
            .expect("rebind");
    // 已经绑过 → 不新建、不报错
    assert!(guard2.is_none(), "已有绑定不应重复进入");
    assert_eq!(workspace2, "/workspace");
    assert_eq!(
        session_env::cached_sandbox_id(conv).as_deref(),
        Some(sandbox_id.as_str()),
        "同会话必须复用同一个沙箱"
    );
    drop(guard2);
    // 解绑 thread-local，才能验证 admin 分支（同一线程已绑定时会复用现有环境）
    drop(guard);

    // --- 2. admin 账号：默认 local ------------------------------------
    let admin_conv = "conv-light-binding-admin";
    let (admin_guard, admin_workspace) = session_env::bind_and_resolve_workspace(
        admin_conv,
        "admin",
        PathBuf::from("/host/admin-workspace"),
    )
    .expect("bind admin session");
    assert!(admin_guard.is_none(), "admin 默认 local，不绑远端");
    assert_eq!(admin_workspace, "/host/admin-workspace");
    assert!(session_env::cached_sandbox_id(admin_conv).is_none());

    // --- 3. 回收 ------------------------------------------------------
    session_env::release(conv);
    assert!(
        session_env::cached_sandbox_id(conv).is_none(),
        "release 后缓存必须清空"
    );
    session_env::release_all();
}
