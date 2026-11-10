//! 私有化扩展缝（ext seam）。
//!
//! 开源仓只提供**装配点**；私有仓作为独立 crate 依赖 `pointer-admin`，
//! 在 `main` 里先 `install()` 一个 [`Ext`] 实现，再调用
//! [`crate::app::build_app`] 装配整棵路由。
//!
//! 默认没有安装任何实现，所有访问器返回空值——所以开源版行为与没有这一层
//! 完全一致。
//!
//! 为什么是进程级安装而不是把 `Arc<dyn Ext>` 塞进 `AppState`：权限列表
//! （`rbac::all_permissions`）和迁移段（`schema::migrate_ext`）都在拿不到
//! state 的位置被调用，把它们全部改成传 state 会牵动十几个调用点。扩展是
//! **进程启动时定一次**的东西，用 `OnceLock` 装一次更贴合语义，也让现有
//! 调用点一行都不用改。
//!
//! 私有仓要做的四件事：
//! 1. `extra_permissions()` 返回自己新增的权限键（如 `payment:read`）；
//! 2. `router()` / `public_router()` 返回自己的路由组；
//! 3. `migrations()` 返回自己的 DDL 段，版本号从 **9000** 起；
//! 4. 在 `main` 里 `pointer_admin::ext::install(Box::new(MyExt))`。

use std::sync::OnceLock;

use axum::Router;

use crate::state::Shared;

/// 私有仓的扩展点。全部方法都有默认实现，只覆写需要的。
pub trait Ext: Send + Sync + 'static {
    /// 额外声明的权限键，会并入 `rbac::all_permissions()`。
    ///
    /// 内置 `admin` 角色自动获得全部；`viewer` 只获得以 `:read` 结尾的。
    fn extra_permissions(&self) -> Vec<String> {
        Vec::new()
    }

    /// 需要登录的管理端路由（挂在 `/api/admin` 下）。
    ///
    /// 拿 `state` 是为了能用开源仓的 `GuardRoute::login_only` / `require`
    /// 给私有路由挂鉴权——扩展路由和内置路由走同一套守卫，不各写一份。
    fn router(&self, _state: &Shared) -> Router<Shared> {
        Router::new()
    }

    /// 免鉴权的公开路由：支付回调、短信发码这类外部 POST。
    ///
    /// 与 `router()` 分开是因为控制面所有 `/api/admin/*` 都要 Cookie 会话，
    /// 而支付平台不会带 Cookie。
    fn public_router(&self) -> Router<Shared> {
        Router::new()
    }

    /// 私有仓的迁移段：`(版本号, DDL)`，版本号从 9000 起，与开源仓的 1..n 不相交。
    fn migrations(&self) -> &'static [(i64, &'static str)] {
        &[]
    }
}

/// 不装任何扩展。开源版用它。
pub struct NoExt;

impl Ext for NoExt {}

static INSTALLED: OnceLock<Box<dyn Ext>> = OnceLock::new();

/// 安装扩展实现。进程内只能装一次。
///
/// 必须在 [`crate::app::build_app`] 之前调用，否则路由和权限都按「没有扩展」
/// 装配，装晚了不生效。
pub fn install(ext: Box<dyn Ext>) -> Result<(), &'static str> {
    INSTALLED.set(ext).map_err(|_| "ext 已经安装过")
}

/// 已安装的扩展；没有则返回 `None`。
pub fn installed() -> Option<&'static dyn Ext> {
    INSTALLED.get().map(|boxed| boxed.as_ref())
}

/// 私有仓额外声明的权限键。没有扩展时为空。
pub fn extra_permissions() -> Vec<String> {
    installed()
        .map(|ext| ext.extra_permissions())
        .unwrap_or_default()
}

/// 需要登录的管理端路由。没有扩展时为空 Router。
pub fn router(state: &Shared) -> Router<Shared> {
    installed()
        .map(|ext| ext.router(state))
        .unwrap_or_default()
}

/// 免鉴权的公开路由。没有扩展时为空 Router。
pub fn public_router() -> Router<Shared> {
    installed()
        .map(|ext| ext.public_router())
        .unwrap_or_default()
}

/// 私有仓的迁移段。没有扩展时为空。
pub fn migrations() -> &'static [(i64, &'static str)] {
    installed().map(|ext| ext.migrations()).unwrap_or(&[])
}

/// 挂进 `AppState` 的私有状态（支付渠道配置、短信服务商配置等）。
///
/// 扩展实现自己持有状态即可；这个空结构体保留给需要按租户区分配置的实现。
#[derive(Debug, Default, Clone)]
pub struct ExtState {}
