//! 工具执行上下文：显式携带「这个工具属于哪个会话、该用哪个执行环境」。
//!
//! 为什么需要它：`Environment` 抽象解决了「主机操作怎么做」，但没解决
//! 「工具拿到的是哪一份实例」。此前靠线程局部（`env::current_env()`）隐式传递，
//! tokio 在 `await` 点换线程、`spawn_blocking` 必然换线程，绑定一丢就会静默
//! 回落到宿主——headless 档位等于失效。
//!
//! 这里的做法：**上下文由 `ToolRegistry::invoke` 的入参显式传入**，
//! 调用期间在**同一线程**上短暂挂成线程局部（`invoke` 是同步函数，工具体内部
//! 不跨 `await`，所以作用域是紧的），工具实现用 `env::require_env()` 取实例
//! ——绑不上就显式报错，不按部署档位兜底；调用返回即恢复，不会把环境泄漏到
//! 线程复用后的下一轮。
//!
//! 工具对模型暴露的 JSON Schema **不包含**这些字段——上下文只存在于执行侧。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use crate::env::{EnvKind, Environment, EnvironmentGuard};

/// 一次工具调用的执行上下文。
#[derive(Clone)]
pub struct ToolContext {
    conversation_id: String,
    session_user_id: String,
    env: Arc<dyn Environment>,
    workspace_root: PathBuf,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("conversation_id", &self.conversation_id)
            .field("session_user_id", &self.session_user_id)
            .field("env_kind", &self.env.kind())
            .field("workspace_root", &self.workspace_root)
            .finish()
    }
}

impl ToolContext {
    pub fn new(
        conversation_id: impl Into<String>,
        session_user_id: impl Into<String>,
        env: Arc<dyn Environment>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            session_user_id: session_user_id.into(),
            env,
            workspace_root: workspace_root.into(),
        }
    }

    /// 按会话解析上下文：先用档位相符的线程绑定，其次用该会话已建好的环境。
    ///
    /// 两边都不符（例如线程上还留着上一个用户的沙箱）就按当前用户重新绑定。
    /// 不按部署档位兜底：沙箱部署下那会给出 `DeniedEnv`，local 部署下会给出与
    /// 会话无关的 `LocalEnv`。
    ///
    /// `fallback_workspace` 只在 local 档位生效——headless 档位的工作区一律是
    /// `env.workspace_root()`（沙箱内的 guest 路径），宿主路径在沙箱里不存在。
    pub fn for_conversation(
        conversation_id: &str,
        session_user_id: &str,
        fallback_workspace: impl Into<PathBuf>,
    ) -> Result<Self> {
        let fallback_workspace = fallback_workspace.into();
        let suits = |env: &Arc<dyn Environment>| {
            crate::env::session_env::env_matches_session_user(env.as_ref(), session_user_id)
        };
        let env = if let Some(env) = crate::env::current_env().filter(&suits).or_else(|| {
            crate::env::session_env::env_for_conversation(conversation_id).filter(&suits)
        }) {
            env
        } else {
            log::info!(
                "tool context: conversation={conversation_id} user={session_user_id} env does not match tier, rebinding"
            );
            let (guard, _) = crate::env::session_env::bind_and_resolve_workspace(
                conversation_id,
                session_user_id,
                fallback_workspace.clone(),
            )?;
            let env = crate::env::current_env().ok_or_else(|| {
                anyhow::anyhow!(
                    "执行环境未绑定，且调用方要求显式绑定：该入口不会按部署档位回落到默认环境"
                )
            })?;
            drop(guard);
            env
        };
        let workspace_root = if env.kind() == EnvKind::Local {
            fallback_workspace
        } else {
            env.workspace_root().to_path_buf()
        };
        Ok(Self::new(
            conversation_id,
            session_user_id,
            env,
            workspace_root,
        ))
    }

    /// 无会话场景（单元测试、工具自检）：显式按宿主处理。
    ///
    /// 只在测试构建里存在：生产路径一律走 [`Self::for_conversation`]，
    /// 免得「没有会话上下文」变成一条静默落到宿主的入口。
    #[cfg(test)]
    pub fn local_default() -> Self {
        let env = crate::env::host_env();
        let workspace_root = env.workspace_root().to_path_buf();
        Self::new("", "", env, workspace_root)
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub fn session_user_id(&self) -> &str {
        &self.session_user_id
    }

    pub fn env(&self) -> Arc<dyn Environment> {
        self.env.clone()
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
}

thread_local! {
    static CURRENT_TOOL_CONTEXT: RefCell<Option<ToolContext>> = const { RefCell::new(None) };
}

/// 工具调用期间挂载上下文 + 环境绑定，`Drop` 时恢复原值。
pub struct ToolContextGuard {
    previous: Option<ToolContext>,
    _env_guard: Option<EnvironmentGuard>,
}

impl ToolContextGuard {
    pub fn enter(ctx: ToolContext) -> Self {
        let env_guard = EnvironmentGuard::enter(ctx.env());
        let previous = CURRENT_TOOL_CONTEXT.with(|slot| slot.borrow_mut().replace(ctx));
        Self {
            previous,
            _env_guard: Some(env_guard),
        }
    }
}

impl Drop for ToolContextGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT_TOOL_CONTEXT.with(|slot| {
            *slot.borrow_mut() = previous;
        });
    }
}

/// 当前正在执行的工具上下文（工具实现需要 conversation_id 时用）。
pub fn current_tool_context() -> Option<ToolContext> {
    CURRENT_TOOL_CONTEXT.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_exposes_context_and_clears_on_drop() {
        assert!(current_tool_context().is_none());
        {
            let ctx = ToolContext::new(
                "conv-1",
                "user-1",
                crate::env::test_light_env(),
                "/workspace",
            );
            let _g = ToolContextGuard::enter(ctx);
            let seen = current_tool_context().expect("上下文应可见");
            assert_eq!(seen.conversation_id(), "conv-1");
            // 同一作用域内工具体读到的环境必须是会话环境，不是宿主
            assert_eq!(crate::env::effective_env().kind(), EnvKind::Headless);
            assert_eq!(
                crate::env::effective_env().workspace_root(),
                Path::new("/data/workspaces")
            );
        }
        assert!(current_tool_context().is_none());
        // 退出后环境绑定也恢复：不会把上一个会话的环境留给线程复用的下一轮
        assert_eq!(crate::env::current_env().is_none(), true);
    }

    #[test]
    fn nested_guard_restores_outer_context() {
        let outer = ToolContext::new("outer", "u", crate::env::test_light_env(), "/workspace");
        let _g1 = ToolContextGuard::enter(outer);
        {
            let inner = ToolContext::new("inner", "u", crate::env::test_light_env(), "/workspace");
            let _g2 = ToolContextGuard::enter(inner);
            assert_eq!(current_tool_context().unwrap().conversation_id(), "inner");
        }
        assert_eq!(current_tool_context().unwrap().conversation_id(), "outer");
    }
}
