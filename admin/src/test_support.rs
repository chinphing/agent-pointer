//! 测试辅助：进程级全局状态（环境变量、HOME 等）的串行化。
//!
//! 环境变量是进程级的，Rust 测试默认多线程并行跑，一个用例 `set_var`
//! 会影响同时运行的其他用例。凡是读写这些全局状态的用例都必须拿同一把锁。

use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 拿全局环境锁。中毒也照样返回，避免一个用例失败把其他用例全带崩。
pub fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
