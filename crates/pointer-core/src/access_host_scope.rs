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
//!
//! 读取约定：**异步阶段一律读 [`current`]**，不要读 `access_host_env` 的
//! thread-local —— agent 循环里的 future 会在 await 点被调度到别的 worker 线程，
//! thread-local 读到的要么是空值，要么是上一个回合的残留。thread-local 只允许
//! 在阻塞边界（`spawn_blocking` 闭包）由 `AccessHostGuard` 落地。

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

    /// 回归（真实故障形态）：请求域名必须活着走到终端的 `spawn_blocking` 边界。
    ///
    /// 复刻 `dispatch/terminal.rs` 的取值方式：async 阶段从 task-local 取值，
    /// 闭包里落成 thread-local，`dotenv::apply_access_host` 再读 thread-local。
    /// 修复前 async 阶段读的是 thread-local —— agent 循环里几十个 await 之后
    /// future 已被调度到别的 worker 线程，取到的就是 `None`，子进程于是没有
    /// `POINTER_ACCESS_HOST`（现场表现：连续四次 `echo` 得到 空/有值/空/旧值）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn access_host_reaches_the_blocking_terminal_boundary() {
        let seen = run_scoped(Some("probe-base.local".into()), || async {
            for _ in 0..64 {
                tokio::task::yield_now().await;
            }
            let host = current();
            tokio::task::spawn_blocking(move || {
                let _guard =
                    crate::access_host_env::AccessHostGuard::enter(host.unwrap_or_default());
                crate::access_host_env::current_access_host()
            })
            .await
            .expect("join")
        })
        .await;
        assert_eq!(seen.as_deref(), Some("probe-base.local"));
    }

    /// 回归：连续多次调用各自取到自己的值，且 guard 出栈后阻塞线程不留残留。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn consecutive_requests_do_not_leak_access_host() {
        for host in ["turn-1.local", "turn-2.local", "turn-3.local"] {
            let (inside, after_drop) = run_scoped(Some(host.into()), || async {
                let host = current();
                tokio::task::spawn_blocking(move || {
                    let inside = {
                        let _guard = crate::access_host_env::AccessHostGuard::enter(
                            host.unwrap_or_default(),
                        );
                        crate::access_host_env::current_access_host()
                    };
                    // guard 在同一线程进、同一线程出：出栈后该线程上不应再有值。
                    (inside, crate::access_host_env::current_access_host())
                })
                .await
                .expect("join")
            })
            .await;
            assert_eq!(inside.as_deref(), Some(host), "host={host}");
            assert_eq!(after_drop, None, "residue leaked for host={host}");
        }
    }

    /// 回归：两个并发请求各带自己的域名，互不覆盖。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_requests_keep_their_own_host() {
        let read = |host: &'static str| {
            tokio::spawn(run_scoped(Some(host.into()), || async {
                for _ in 0..64 {
                    tokio::task::yield_now().await;
                }
                let host = current();
                tokio::task::spawn_blocking(move || {
                    let _guard = crate::access_host_env::AccessHostGuard::enter(
                        host.unwrap_or_default(),
                    );
                    crate::access_host_env::current_access_host()
                })
                .await
                .expect("join")
            }))
        };

        let a = read("a.local");
        let b = read("b.local");
        assert_eq!(a.await.expect("a").as_deref(), Some("a.local"));
        assert_eq!(b.await.expect("b").as_deref(), Some("b.local"));
    }

    /// 契约：thread-local 不跨线程。所以 async 阶段只能从 task-local 取值，
    /// 在阻塞线程上进的 guard 也只在那个线程可见。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn thread_local_does_not_cross_threads() {
        let on_blocking = tokio::task::spawn_blocking(|| {
            let _guard = crate::access_host_env::AccessHostGuard::enter("blocking.local".into());
            crate::access_host_env::current_access_host()
        })
        .await
        .expect("join");
        assert_eq!(on_blocking.as_deref(), Some("blocking.local"));
        // 回到 async 任务：阻塞线程上的 guard 不可见。
        assert_eq!(crate::access_host_env::current_access_host(), None);
    }
}
