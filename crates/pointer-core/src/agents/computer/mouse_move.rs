//! Mouse movement planning/execution abstraction for computer agent backends.
//!
//! Geometry (waypoints) and timing (per-step delays) are planned separately, matching
//! the Python `MouseMove._generate_path` / `_calculate_intervals` split.

use super::timing::{
    MOUSE_MOVE_LINEAR_STEP_MAX_PX, MOUSE_MOVE_STEP_DURATION_SECS,
    MOUSE_MOVE_TOTAL_DURATION_SECS,
};
use std::time::Duration;

/// Planned movement geometry only (excludes the current cursor point).
#[derive(Debug, Clone)]
pub struct MouseMovePath {
    pub points: Vec<(i32, i32)>,
}

/// Per-step sleep durations after each waypoint move (seconds).
#[derive(Debug, Clone)]
pub struct MouseMoveTimingPlan {
    pub step_intervals_secs: Vec<f64>,
}

/// Combined path + timing for execution.
#[derive(Debug, Clone)]
pub struct MouseMovePlan {
    pub path: MouseMovePath,
    pub timing: MouseMoveTimingPlan,
}

/// Planned movement strategy kind (geometry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMoveStrategy {
    /// Straight line with uniform spatial stepping.
    LinearUniform,
}

/// How [`MouseMoveTimingConfig::duration_secs`] is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMoveDurationMode {
    /// Fixed sleep after each waypoint (`duration_secs` per step).
    Step,
    /// Eased intervals summing to `duration_secs` across all waypoints.
    Total,
}

/// Tunables for path (geometry) planning.
#[derive(Debug, Clone, Copy)]
pub struct MouseMovePathConfig {
    pub strategy: MouseMoveStrategy,
    /// Maximum pixel distance between consecutive points.
    pub max_step_px: f64,
}

impl Default for MouseMovePathConfig {
    fn default() -> Self {
        Self {
            strategy: MouseMoveStrategy::LinearUniform,
            max_step_px: MOUSE_MOVE_LINEAR_STEP_MAX_PX,
        }
    }
}

/// Tunables for timing (interval) planning.
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveTimingConfig {
    pub duration_mode: MouseMoveDurationMode,
    /// Total move time (seconds) when [`MouseMoveDurationMode::Total`].
    pub total_duration_secs: f64,
    /// Per-step interval (seconds) when [`MouseMoveDurationMode::Step`].
    pub step_duration_secs: f64,
    /// Use ease-in-out instead of ease-out when in total mode.
    pub ease_in_out: bool,
}

impl Default for MouseMoveTimingConfig {
    fn default() -> Self {
        Self {
            duration_mode: MouseMoveDurationMode::Total,
            total_duration_secs: MOUSE_MOVE_TOTAL_DURATION_SECS,
            step_duration_secs: MOUSE_MOVE_STEP_DURATION_SECS,
            ease_in_out: false,
        }
    }
}

/// Full movement planner configuration.
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveConfig {
    pub path: MouseMovePathConfig,
    pub timing: MouseMoveTimingConfig,
}

impl Default for MouseMoveConfig {
    fn default() -> Self {
        Self {
            path: MouseMovePathConfig::default(),
            timing: MouseMoveTimingConfig::default(),
        }
    }
}

/// Plans cursor geometry (waypoints).
#[derive(Debug, Clone, Copy)]
pub struct MouseMovePathPlanner {
    config: MouseMovePathConfig,
}

impl MouseMovePathPlanner {
    pub fn new(config: MouseMovePathConfig) -> Self {
        Self { config }
    }

    pub fn plan(&self, from: (i32, i32), to: (i32, i32)) -> MouseMovePath {
        match self.config.strategy {
            MouseMoveStrategy::LinearUniform => MouseMovePath {
                points: plan_linear_uniform_path(from, to, self.config.max_step_px),
            },
        }
    }
}

/// Plans per-step sleep intervals for a path with `num_steps` waypoints.
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveTimingPlanner {
    config: MouseMoveTimingConfig,
}

impl MouseMoveTimingPlanner {
    pub fn new(config: MouseMoveTimingConfig) -> Self {
        Self { config }
    }

    pub fn plan(&self, num_steps: usize) -> MouseMoveTimingPlan {
        let step_intervals_secs = match self.config.duration_mode {
            MouseMoveDurationMode::Step => {
                vec![self.config.step_duration_secs; num_steps]
            }
            MouseMoveDurationMode::Total => {
                if self.config.ease_in_out {
                    ease_in_out_intervals(num_steps, self.config.total_duration_secs)
                } else {
                    ease_out_intervals(num_steps, self.config.total_duration_secs)
                }
            }
        };
        MouseMoveTimingPlan { step_intervals_secs }
    }
}

/// Composes path and timing planners (Python `MouseMove` equivalent).
#[derive(Debug, Clone, Copy)]
pub struct MouseMovePlanner {
    path: MouseMovePathPlanner,
    timing: MouseMoveTimingPlanner,
}

impl MouseMovePlanner {
    pub fn new(config: MouseMoveConfig) -> Self {
        Self {
            path: MouseMovePathPlanner::new(config.path),
            timing: MouseMoveTimingPlanner::new(config.timing),
        }
    }

    pub fn plan(&self, from: (i32, i32), to: (i32, i32)) -> MouseMovePlan {
        let path = self.path.plan(from, to);
        let timing = self.timing.plan(path.points.len());
        MouseMovePlan { path, timing }
    }
}

/// Execute a movement plan by repeatedly invoking backend `move_abs`.
pub fn execute_move_plan<E>(
    plan: &MouseMovePlan,
    mut move_abs: impl FnMut(i32, i32) -> Result<(), E>,
) -> Result<(), E> {
    debug_assert_eq!(
        plan.path.points.len(),
        plan.timing.step_intervals_secs.len()
    );
    for ((x, y), dt) in plan
        .path
        .points
        .iter()
        .copied()
        .zip(plan.timing.step_intervals_secs.iter().copied())
    {
        move_abs(x, y)?;
        if dt.is_finite() && dt > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(dt));
        }
    }
    Ok(())
}

/// Ease-out: `n` intervals (seconds) sum to `total_time` (Python `_ease_out_intervals`).
pub fn ease_out_intervals(n: usize, total_time: f64) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![total_time];
    }
    let n_f = n as f64;
    (0..n)
        .map(|i| {
            let progress_after = (i + 1) as f64 / n_f;
            let progress_before = i as f64 / n_f;
            let t_after = 1.0 - (1.0 - progress_after).max(0.0).sqrt();
            let t_before = 1.0 - (1.0 - progress_before).max(0.0).sqrt();
            total_time * (t_after - t_before)
        })
        .collect()
}

/// Ease-in-out: slow start/end, faster middle (Python `_ease_in_out_intervals`).
pub fn ease_in_out_intervals(n: usize, total_time: f64) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![total_time];
    }
    let n_f = n as f64;
    (0..n)
        .map(|i| {
            let progress_after = (i + 1) as f64 / n_f;
            let progress_before = i as f64 / n_f;
            let t_after = progress_to_ease_in_out(progress_after);
            let t_before = progress_to_ease_in_out(progress_before);
            total_time * (t_after - t_before)
        })
        .collect()
}

fn progress_to_ease_in_out(p: f64) -> f64 {
    if p <= 0.5 {
        2.0 * p * p
    } else {
        1.0 - 2.0 * (1.0 - p) * (1.0 - p)
    }
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
    fn path_and_timing_planners_same_length() {
        let planner = MouseMovePlanner::new(MouseMoveConfig::default());
        let plan = planner.plan((0, 0), (200, 0));
        assert_eq!(
            plan.path.points.len(),
            plan.timing.step_intervals_secs.len()
        );
    }

    #[test]
    fn ease_out_intervals_sum_to_total() {
        let intervals = ease_out_intervals(10, 0.5);
        assert_eq!(intervals.len(), 10);
        let sum: f64 = intervals.iter().sum();
        assert!((sum - 0.5).abs() < 1e-6);
    }

    #[test]
    fn ease_out_starts_faster_than_end() {
        let intervals = ease_out_intervals(5, 1.0);
        assert!(intervals[0] < intervals[4]);
    }

    #[test]
    fn step_mode_uses_fixed_intervals() {
        let timing = MouseMoveTimingPlanner::new(MouseMoveTimingConfig {
            duration_mode: MouseMoveDurationMode::Step,
            step_duration_secs: 0.03,
            ..Default::default()
        })
        .plan(4);
        assert_eq!(timing.step_intervals_secs, vec![0.03; 4]);
    }

    #[test]
    fn ease_in_out_middle_larger_than_ends() {
        let intervals = ease_in_out_intervals(5, 1.0);
        assert!(intervals[2] > intervals[0]);
        assert!(intervals[2] > intervals[4]);
    }

    #[test]
    fn execute_move_plan_calls_all_points() {
        let plan = MouseMovePlan {
            path: MouseMovePath {
                points: vec![(1, 1), (2, 2), (3, 3)],
            },
            timing: MouseMoveTimingPlan {
                step_intervals_secs: vec![0.0, 0.0, 0.0],
            },
        };
        let mut seen = Vec::new();
        execute_move_plan(&plan, |x, y| -> Result<(), ()> {
            seen.push((x, y));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, plan.path.points);
    }
}
