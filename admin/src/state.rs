//! 进程内共享状态。

use std::sync::Arc;

use crate::config::AdminConfig;
use crate::db::AdminStore;
use crate::oauth::code::CodeStore;

pub struct AppState {
    pub store: AdminStore,
    pub config: AdminConfig,
    pub codes: CodeStore,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn new(store: AdminStore, config: AdminConfig) -> Shared {
        Arc::new(Self {
            store,
            config,
            codes: CodeStore::default(),
        })
    }
}
