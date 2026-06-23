//! External work_items store (atomic units outside board JSON).

pub mod model;
pub mod persistence;
pub mod store;

pub use model::{
    BatchStats, CampaignStats, SeedOutcome, WorkItem, WorkItemDraft, WorkItemStatus,
    MAX_INLINE_SEED,
};
pub use persistence::WorkItemSqlite;
pub use store::{
    claim_from_value, delta_from_value, WorkItemClaim, WorkItemDelta, WorkItemStore,
    WORK_ITEM_INJECT_IN_PROGRESS, WORK_ITEM_INJECT_NEXT_READY, WORK_ITEM_INJECT_RECENT_DONE,
};

pub fn open_default_persistence() -> Option<std::sync::Arc<WorkItemSqlite>> {
    let dir = crate::storage::app_data_dir().ok()?;
    let path = dir.join("work_items.db");
    WorkItemSqlite::open(path).ok()
}
