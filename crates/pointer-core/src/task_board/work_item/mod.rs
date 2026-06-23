//! External work_items store (atomic units outside board JSON).

pub mod api;
pub mod export;
pub mod export_tool;
pub mod import;
pub mod model;
pub mod persistence;
pub mod store;

pub use api::{list_work_items_json, work_item_stats_json};
pub use export::{export_work_items, ExportOutcome, ExportRequest};
pub use import::drafts_from_source_value;
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
