//! Mouse movement planning/execution abstraction for computer agent backends.
//!
//! Current implementation keeps one strategy: straight-line uniform movement.
//! The structure is intentionally split into:
//! - planner (target -> waypoints)
//! - executor (waypoints -> backend calls)
//! so new strategies (Bezier, easing, jitter, obstacle-aware) can be added
//! without changing action call sites.

use super::timing::{
    MOUSE_MOVE_LINEAR_STEP_INTERVAL_MS, MOUSE_MOVE_LINEAR_STEP_MAX_PX,
};
use std::time::Duration;

/// Planned movement strategy kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMoveStrategy {
    /// Straight line with uniform spatial stepping.
    LinearUniform,
}

/// Tunables for movement planner and executor.
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveConfig {
    pub strategy: MouseMoveStrategy,
    /// Maximum pixel distance between consecutive points.
    pub max_step_px: f64,
    /// Sleep duration between consecutive points.
    pub step_interval_ms: u64,
}

impl Default for MouseMoveConfig {
    fn default() -> Self {
        Self {
            strategy: MouseMoveStrategy::LinearUniform,
            max_step_px: MOUSE_MOVE_LINEAR_STEP_MAX_PX,
            step_interval_ms: MOUSE_MOVE_LINEAR_STEP_INTERVAL_MS,
        }
    }
}

/// Concrete movement path, excluding the current cursor point.
#[derive(Debug, Clone)]
pub struct MouseMovePlan {
    pub strategy: MouseMoveStrategy,
    pub points: Vec<(i32, i32)>,
    pub step_interval_ms: u64,
}

/// Planner for cursor movement.
#[derive(Debug, Clone, Copy)]
pub struct MouseMovePlanner {
    config: MouseMoveConfig,
}

impl MouseMovePlanner {
    pub fn new(config: MouseMoveConfig) -> Self {
        Self { config }
    }

    pub fn plan(&self, from: (i32, i32), to: (i32, i32)) -> MouseMovePlan {
        match self.config.strategy {
            MouseMoveStrategy::LinearUniform => {
                let points = plan_linear_uniform_path(from, to, self.config.max_step_px);
                MouseMovePlan {
                    strategy: self.config.strategy,
                    points,
                    step_interval_ms: self.config.step_interval_ms,
                }
            }
        }
    }
}

/// Execute a movement plan by repeatedly invoking backend `move_abs`.
pub fn execute_move_plan<E>(
    plan: &MouseMovePlan,
    mut move_abs: impl FnMut(i32, i32) -> Result<(), E>,
) -> Result<(), E> {
    for (idx, (x, y)) in plan.points.iter().copied().enumerate() {
        move_abs(x, y)?;
        if idx + 1 < plan.points.len() && plan.step_interval_ms > 0 {
            std::thread::sleep(Duration::from_millis(plan.step_interval_ms));
        }
    }
    Ok(())
}

fn plan_linear_uniform_path(from: (i32, i32), to: (i32, i32), max_step_px: f64) -> Vec<(i32, i32)> {
    if from == to {
        return Vec::new();
    }

    let max_step = if max_step_px.is_finite() && max_step_px > 0.0 {
        max_step_px
    } else {
        MOUSE_MOVE_LINEAR_STEP_MAX_PX
    };

    let dx = (to.0 - from.0) as f64;
    let dy = (to.1 - from.1) as f64;
    let distance = (dx * dx + dy * dy).sqrt();
    let steps = (distance / max_step).ceil().max(1.0) as usize;

    let mut out = Vec::with_capacity(steps);
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = from.0 as f64 + dx * t;
        let y = from.1 as f64 + dy * t;
        out.push((x.round() as i32, y.round() as i32));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_uniform_returns_empty_for_same_point() {
        let p = plan_linear_uniform_path((10, 10), (10, 10), 12.0);
        assert!(p.is_empty());
    }

    #[test]
    fn linear_uniform_ends_at_target() {
        let p = plan_linear_uniform_path((0, 0), (100, 0), 12.0);
        assert!(!p.is_empty());
        assert_eq!(p.last().copied(), Some((100, 0)));
    }

    #[test]
    fn execute_move_plan_calls_all_points() {
        let plan = MouseMovePlan {
            strategy: MouseMoveStrategy::LinearUniform,
            points: vec![(1, 1), (2, 2), (3, 3)],
            step_interval_ms: 0,
        };
        let mut seen = Vec::new();
        execute_move_plan(&plan, |x, y| -> Result<(), ()> {
            seen.push((x, y));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, plan.points);
    }
}
