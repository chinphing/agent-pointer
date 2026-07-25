//! Dependency gate for parent board milestones.

use super::super::model::{BoardDocument, ItemStatus};
use super::super::state_machine::dependencies_satisfied;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyCheck {
    Ready,
    Blocked { reason: String },
}

pub fn check_dependencies(doc: &BoardDocument, item_id: &str) -> DependencyCheck {
    let Some(item) = doc.global_milestones.iter().find(|i| i.id == item_id) else {
        return DependencyCheck::Blocked {
            reason: format!("unknown item_id {item_id}"),
        };
    };
    if dependencies_satisfied(doc, item) {
        DependencyCheck::Ready
    } else {
        let blocking: Vec<String> = item
            .depends_on
            .iter()
            .filter(|dep| {
                !doc.global_milestones.iter().any(|row| {
                    row.id == **dep
                        && matches!(row.status, ItemStatus::Done | ItemStatus::Cancelled)
                })
            })
            .cloned()
            .collect();
        DependencyCheck::Blocked {
            reason: if blocking.is_empty() {
                "dependencies not satisfied".into()
            } else {
                format!("waiting on: {}", blocking.join(", "))
            },
        }
    }
}

pub fn mark_ready_after_report(doc: &mut BoardDocument) {
    super::super::state_machine::mark_ready_pending_rows(doc);
}
