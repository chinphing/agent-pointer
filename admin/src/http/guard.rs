//! 路由级鉴权。
//!
//! 以前每个 handler 开头都要写两行：
//!
//! ```ignore
//! let auth = current_auth(&state, &headers).await?;  // 401
//! auth.require("release:read")?;                     // 403
//! ```
//!
//! 45 个 handler 全这么写，于是「这条路由要什么权限」只有翻进函数体才知道，
//! 漏写一行也只在出事时才发现（`/api/admin/releases*` 就是这么漏掉的）。
//!
//! 现在改成在路由表上声明：
//!
//! ```ignore
//! .route("/api/admin/releases", get(releases::admin_list).require(&state, "release:read"))
//! .route("/api/admin/my-wallet", get(billing::my_wallet).login_only(&state))
//! ```
//!
//! 中间件负责「验登录 + 验权限」，并把 `AdminAuth` 塞进请求扩展；
//! handler 需要当前用户时取 `Extension<AdminAuth>` 即可，不再自己验一遍。

use axum::extract::{Request, State};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::IntoResponse;
use axum::routing::MethodRouter;

use super::auth::current_auth;
use crate::state::Shared;

/// 路由级鉴权声明。
pub trait GuardRoute {
    /// 只要登录，不校验具体权限。
    ///
    /// 资源类接口用这个：权限取决于运行时才知道的资源类型（`perms_for(kind)`），
    /// 只能在 handler 里判，路由层兜住「必须登录」这一层。
    fn login_only(self, state: &Shared) -> Self;

    /// 登录 + 指定权限。权限键与迁移前 handler 里的写法保持一致。
    fn require(self, state: &Shared, perm: &'static str) -> Self;
}

impl GuardRoute for MethodRouter<Shared> {
    fn login_only(self, state: &Shared) -> Self {
        self.guard(state, None)
    }

    fn require(self, state: &Shared, perm: &'static str) -> Self {
        self.guard(state, Some(perm))
    }
}

/// 内部实现：`login_only` 与 `require` 只差一个 `Option`。
trait GuardExt {
    fn guard(self, state: &Shared, perm: Option<&'static str>) -> Self;
}

impl GuardExt for MethodRouter<Shared> {
    fn guard(self, state: &Shared, perm: Option<&'static str>) -> Self {
        let st = state.clone();
        self.layer(from_fn_with_state(
            st,
            move |State(state): State<Shared>, mut req: Request, next: Next| async move {
                let auth = match current_auth(&state, req.headers()).await {
                    Ok(auth) => auth,
                    // 未登录 / 会话过期
                    Err(e) => return e.into_response(),
                };
                if let Some(perm) = perm {
                    if let Err(e) = auth.require(perm) {
                        return e.into_response();
                    }
                }
                // 交给 handler 用；这里插入的是已经验过的身份，不会再验第二次。
                req.extensions_mut().insert(auth);
                next.run(req).await
            },
        ))
    }
}
