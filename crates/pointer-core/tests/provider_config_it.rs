//! 部署配置 → 档位解析：只认新值（`local` / `headless` / `desktop`）。
//!
//! 走的是应用真实读取路径：`server_config::environment_*()` → `ProviderConfig::from_env()`
//! → `validate_deployment()` / `resolve()`。旧值 `light` / `container` 已删除，老配置
//! 必须在 `from_env()` 就失败，且错误里点名该改用哪个新值。
//!
//! 所有断言放在**同一个测试函数**里顺序执行：环境变量是进程级的，分开写会在并行
//! 测试之间互相踩。

use pointer_core::env::provider::{Backend, ProviderConfig, Tier};

/// 清掉可能影响本测试的 ambient 配置，避免开发机上的真实配置干扰断言。
fn reset_env() {
    for key in [
        "POINTER_SERVER_ENVIRONMENT_ACCOUNTS",
        "POINTER_SERVER_ENVIRONMENT_DESKTOP",
        "POINTER_SERVER_ADMIN_ACCOUNTS",
    ] {
        std::env::remove_var(key);
    }
}

#[test]
fn tier_values_drive_startup_and_legacy_values_fail_fast() {
    reset_env();
    std::env::set_var(
        "POINTER_SERVER_ENVIRONMENT_SANDBOX_URL",
        "http://127.0.0.1:3000",
    );

    // 1) 新值：provider = "headless" + [environment.backends] headless = "docker"
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "headless");
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_BACKENDS", "headless=docker");
    let cfg = ProviderConfig::from_env().expect("headless 配置必须能解析");
    cfg.validate_deployment()
        .expect("headless 部署必须通过启动校验");
    let resolved = cfg
        .resolve(Some("alice"))
        .expect("headless 必须能解析出环境");
    assert_eq!(resolved.tier, Tier::Headless);
    assert_eq!(
        resolved.backend,
        Backend::Docker,
        "[environment.backends] headless = \"docker\" 必须生效"
    );

    // 2) 新值：provider = "desktop" + backends desktop = "docker"
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "desktop");
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_BACKENDS", "desktop=docker");
    let cfg = ProviderConfig::from_env().expect("desktop 配置必须能解析");
    cfg.validate_deployment()
        .expect("desktop 部署必须通过启动校验");
    let resolved = cfg
        .resolve(Some("alice"))
        .expect("desktop 必须能解析出环境");
    assert_eq!(resolved.tier, Tier::Desktop);
    assert_eq!(resolved.backend, Backend::Docker);

    // 3) 旧值：provider = "light" → 启动即失败，并提示改用 headless
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "light");
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_BACKENDS", "light=docker");
    let err = ProviderConfig::from_env()
        .expect_err("旧值 light 必须解析失败")
        .to_string();
    assert!(err.contains("headless"), "错误必须提示新值 headless：{err}");
    assert!(err.contains("旧值已废弃"), "错误必须说明旧值已废弃：{err}");

    // 4) 旧值：provider = "container" → 启动即失败，并提示改用 desktop
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "container");
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_BACKENDS", "container=docker");
    let err = ProviderConfig::from_env()
        .expect_err("旧值 container 必须解析失败")
        .to_string();
    assert!(err.contains("desktop"), "错误必须提示新值 desktop：{err}");
    assert!(err.contains("旧值已废弃"), "错误必须说明旧值已废弃：{err}");

    // 5) 旧值只出现在 [environment.backends] 的键里，同样要失败并带上迁移提示
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "headless");
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_BACKENDS", "light=docker");
    let err = ProviderConfig::from_env()
        .expect_err("backends 里的旧值 light 必须解析失败")
        .to_string();
    assert!(err.contains("headless"), "错误必须提示新值 headless：{err}");
    assert!(err.contains("旧值已废弃"), "错误必须说明旧值已废弃：{err}");

    for key in [
        "POINTER_SERVER_ENVIRONMENT_PROVIDER",
        "POINTER_SERVER_ENVIRONMENT_BACKENDS",
        "POINTER_SERVER_ENVIRONMENT_SANDBOX_URL",
    ] {
        std::env::remove_var(key);
    }
}
