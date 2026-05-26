//! Per-tier tool prompt bodies for computer desktop tools (index vs coordinate).

use crate::agents::computer::tier::ComputerTier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerPositioningMode {
    Index,
    Coordinate,
}

pub fn positioning_mode_for_tier(tier: ComputerTier) -> Option<ComputerPositioningMode> {
    match tier {
        ComputerTier::Primary => None,
        ComputerTier::Intermediate => Some(ComputerPositioningMode::Index),
        ComputerTier::Advanced => Some(ComputerPositioningMode::Coordinate),
    }
}

const INDEX_MOUSE: &str = include_str!("prompts/index/mouse.md");
const INDEX_COMPOSITE: &str = include_str!("prompts/index/composite_action.md");
const INDEX_MODIFIED: &str = include_str!("prompts/index/modified_click.md");
const COORD_MOUSE: &str = include_str!("prompts/coordinate/mouse.md");
const COORD_COMPOSITE: &str = include_str!("prompts/coordinate/composite_action.md");
const COORD_MODIFIED: &str = include_str!("prompts/coordinate/modified_click.md");

/// Tool names that have index vs coordinate prompt variants.
pub fn computer_tool_doc_override(tool_name: &str, mode: ComputerPositioningMode) -> Option<&'static str> {
    match (tool_name, mode) {
        ("mouse", ComputerPositioningMode::Index) => Some(INDEX_MOUSE),
        ("mouse", ComputerPositioningMode::Coordinate) => Some(COORD_MOUSE),
        ("composite_action", ComputerPositioningMode::Index) => Some(INDEX_COMPOSITE),
        ("composite_action", ComputerPositioningMode::Coordinate) => Some(COORD_COMPOSITE),
        ("modified_click", ComputerPositioningMode::Index) => Some(INDEX_MODIFIED),
        ("modified_click", ComputerPositioningMode::Coordinate) => Some(COORD_MODIFIED),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_has_no_bound_mode() {
        assert_eq!(
            positioning_mode_for_tier(ComputerTier::Primary),
            None
        );
    }
}
