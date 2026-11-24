//! local 档位的会话绑定（**不需要**沙箱网关，可常规跑）。
//!
//! ```bash
//! cargo test -p pointer-core --test local_binding_it
//! ```
//!
//! 回归：部署配成非 local 档位（这里用 desktop）时，`[auth] admins` 里的账号仍
//! 解析为 local 档位。以前 local 档位什么都不绑：
//!
//! - 工具线程（`spawn_blocking`）按会话重放时查不到环境 → `effective_env()`
//!   按部署档位返回 `DeniedEnv` → `terminal` 报「工作区目录无效或不存在」；
//! - 报错指向路径，真实原因（环境没绑定）完全看不出来。
//!
//! 现在 local 档位也显式绑 `LocalEnv`（工作区 = 宿主路径），并进会话缓存供重放。

use pointer_core::env::{current_env, effective_env, session_env, EnvKind};

#[test]
fn admin_on_desktop_deployment_binds_a_replayable_host_env() {
    // 部署档位 desktop + docker 后端：admin 走 local，所以不会真的起容器。
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "desktop");
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_BACKEND", "docker");
    std::env::set_var("POINTER_SERVER_ADMIN_ACCOUNTS", "admin");
    std::env::remove_var("POINTER_SERVER_ENVIRONMENT_ACCOUNTS");

    let app = tempfile::tempdir().expect("app data");
    std::env::set_var("POINTER_APP_DATA_DIR", app.path());
    let dir = tempfile::tempdir().expect("tempdir");
    let conv = "conv-local-binding-it";

    let (guard, workspace) =
        session_env::bind_and_resolve_workspace(conv, "admin", dir.path().to_path_buf())
            .expect("bind admin session");

    assert!(guard.is_some(), "local 档位也必须绑定环境");
    let selected = dir.path().canonicalize().unwrap_or_else(|_| dir.path().to_path_buf());
    let bound = std::path::PathBuf::from(&workspace);
    let bound = bound.canonicalize().unwrap_or(bound);
    assert_eq!(
        bound, selected,
        "用户选中的目录就是这次会话的工作区：{workspace}"
    );
    assert_eq!(effective_env().kind(), EnvKind::Local);
    assert!(!effective_env().is_denied(), "local 档位不是「环境未绑定」");
    assert!(
        session_env::cached_sandbox_id(conv).is_none(),
        "admin 是 local 档位，不该建沙箱"
    );

    // 工具线程只有会话 id（没有 thread-local）：必须能重放出来。
    let replay_conv = conv.to_string();
    let ws_for_thread = workspace.clone();
    let replayed_workspace = std::thread::spawn(move || {
        assert!(current_env().is_none(), "新线程不该有 thread-local 绑定");
        let _g = session_env::bind_cached(&replay_conv)
            .expect("回归：local 档位必须能按会话重放（以前返回 None → DeniedEnv）");
        assert_eq!(current_env().unwrap().kind(), EnvKind::Local);
        assert!(!effective_env().is_denied());
        effective_env().workspace_root().display().to_string()
    })
    .join()
    .expect("join");
    assert_eq!(replayed_workspace, ws_for_thread);

    drop(guard);
    session_env::release(conv);
    assert!(
        session_env::env_for_conversation(conv).is_none(),
        "release 必须清掉 local 缓存"
    );
}
