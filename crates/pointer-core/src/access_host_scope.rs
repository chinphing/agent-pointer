//! 请求级域名作用域（tokio task-local）。
//!
//! `access_host_env` 里的 thread-local 解决的是"子进程要拿到值"，但值本身
//! 从 HTTP handler 走到 agent 循环要跨越 `tokio::spawn`，thread-local 跟不过去。
//! 这里用 task-local 承载，模式与 `web_request_auth` 一致。
//!
//! 三个边界必须显式重放（task-local 不会自动继承）：
//! - 后台 terminal 任务（`dispatch/terminal.rs`）
//! - 后台子智能体任务（`run_subagent_delegation.rs`）
//! - `spawn_blocking` 里的终端进程

use std::future::Future;

tokio::task_local! {
    static SCOPED_ACCESS_HOST: String;
}/// 在 `host` 作用域内运行。`None` / 空串表示"没有域名上下文"，
/// 此时 `current()` 返回 `None`，脚本侧应自行回退。
pub async fn run_scoped<F, Fut, T>(host: Option<String>, f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    SCOPED_ACCESS_HOST
        .scope(host.unwrap_or_default(), f())
        .await
}

/// 当前作用域内的域名；不在作用域内或值为空时返回 `None`。
pub fn current() -> Option<String> {
    SCOPED_ACCESS_HOST
        .try_with(|host| host.clone())
        .ok()
        .filter(|host| !host.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exposes_host_inside_the_scope() {
        let seen = run_scoped(Some("pointer.internal".into()), || async {
            current()
        })
        .await;
        assert_eq!(seen.as_deref(), Some("pointer.internal"));
    }

    #[tokio::test]
    async fn empty_or_missing_host_reads_as_none() {
        assert_eq!(run_scoped(None, || async { current() }).await, None);
        assert_eq!(run_scoped(Some("   ".into()), || async { current() }).await, None);
    }

    #[tokio::test]
    async fn value_is_absent_outside_the_scope() {
        assert_eq!(current(), None);
    }
}
