//! 私有化扩展缝（ext seam）。
//!
//! 开源仓只在这里提供**装配点**；私有仓在自己的分支里往这些函数里塞实现。
//! 默认实现全部是空的——不注册路由、不加权限、不跑迁移，所以开源版行为
//! 与没有这一层完全一致。
//!
//! 私有仓要做的三件事：
//! 1. `extra_permissions()` 返回自己新增的权限键（如 `payment:read`）；
//! 2. `router()` / `public_router()` 返回自己的路由组；
//! 3. `migrations()` 返回自己的 DDL 段，版本号从 **9000** 起。

use axum::Router;

use crate::state::Shared;

/// 私有仓额外声明的权限键。
///
/// 会并入 `rbac::all_permissions()`，因此内置的 `admin` / `viewer` 角色
/// 会自动获得它们（`viewer` 只拿以 `:read` 结尾的）。
pub fn extra_permissions() -> Vec<String> {
    Vec::new()
}

/// 需要登录的管理端路由（挂在 `/api/admin` 下）。
pub fn router() -> Router<Shared> {
    Router::new()
}

/// 免鉴权的公开路由：支付回调、短信发码这类外部 POST。
///
/// 与 `router()` 分开是因为控制面所有 `/api/admin/*` 都要 Cookie 会话，
/// 而支付平台不会带 Cookie。
pub fn public_router() -> Router<Shared> {
    Router::new()
}

/// 私有仓的迁移段：`(版本号, DDL)`，版本号从 9000 起，与开源仓的 1..n 不相交。
pub fn migrations() -> &'static [(i64, &'static str)] {
    &[]
}

/// 挂进 `AppState` 的私有状态（支付渠道配置、短信服务商配置等）。
#[derive(Debug, Default, Clone)]
pub struct ExtState {}
