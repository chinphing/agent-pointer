//! 进程内共享状态。
//!
//! 刻意**不**在这里放任何「跨请求要活下来」的业务状态：
//! 授权码已落库（`oauth_codes`），会话在库里，包在磁盘上。
//! 这样多实例部署时各进程之间没有隐式耦合。

use std::sync::Arc;

use crate::config::AdminConfig;
use crate::db::AdminStore;

pub struct AppState {
    pub store: AdminStore,
    pub config: AdminConfig,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn new(store: AdminStore, config: AdminConfig) -> Shared {
        Arc::new(Self { store, config })
    }
}
