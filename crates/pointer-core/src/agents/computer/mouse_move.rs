//! Mouse movement planning/execution abstraction for computer agent backends.
//!
//! Geometry (waypoints) and timing (per-step delays) are planned separately, matching
//! the Python `MouseMove._generate_path` / `_calculate_intervals` split.
//!
//! Default path: straight line with a fixed point count (10), uniform `t`; total time 0.5s ease-out.
//! See `docs/design/computer-mouse-movement-roadmap.md`.

use super::timing::{MOUSE_MOVE_DEFAULT_POINT_COUNT, MOUSE_MOVE_TOTAL_DURATION_SECS};
use log::debug;
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
    /// Straight line with `point_count` uniformly spaced waypoints (Python default ~10).
    LinearByPointCount,
}

/// Tunables for path (geometry) planning.
#[derive(Debug, Clone, Copy)]
pub struct MouseMovePathConfig {
    pub strategy: MouseMoveStrategy,
    /// Number of waypoints along the segment (`from` excluded, ends at `to`).
    pub point_count: usize,
}

impl Default for MouseMovePathConfig {
    fn default() -> Self {
        Self {
            strategy: MouseMoveStrategy::LinearByPointCount,
            point_count: MOUSE_MOVE_DEFAULT_POINT_COUNT,
        }
    }
}

/// Tunables for timing (interval) planning.
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveTimingConfig {
    /// Total move time (seconds), split across waypoints with easing.
    pub total_duration_secs: f64,
    /// Use ease-in-out instead of ease-out.
    pub ease_in_out: bool,
}

impl Default for MouseMoveTimingConfig {
    fn default() -> Self {
        Self {
            total_duration_secs: MOUSE_MOVE_TOTAL_DURATION_SECS,
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
            MouseMoveStrategy::LinearByPointCount => MouseMovePath {
                points: plan_linear_by_point_count(from, to, self.config.point_count),
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
        let step_intervals_secs = if self.config.ease_in_out {
            ease_in_out_intervals(num_steps, self.config.total_duration_secs)
        } else {
            ease_out_intervals(num_steps, self.config.total_duration_secs)
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

/// Execute a movement plan by repeatedly invoking backend `move_abs`, then one more move at
/// `target` so the OS / apps refresh hit-testing on the terminal pixel.
pub fn execute_move_plan<E>(
    plan: &MouseMovePlan,
    target: (i32, i32),
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
    move_abs(target.0, target.1)?;
    debug!(
        "mouse move: final perception event at ({}, {}) ({} path points)",
        target.0,
        target.1,
        plan.path.points.len()
    );
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

/// `point_count` waypoints along a straight line; `t = i / n` for `i = 1..=n` (ends at `to`).
fn plan_linear_by_point_count(
    from: (i32, i32),
    to: (i32, i32),
    point_count: usize,
) -> Vec<(i32, i32)> {
    if from == to || point_count == 0 {
        return Vec::new();
    }
    let n = point_count.max(1);
    let dx = (to.0 - from.0) as f64;
    let dy = (to.1 - from.1) as f64;
    let mut out = Vec::with_capacity(n);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let x = from.0 as f64 + dx * t;
        let y = from.1 as f64 + dy * t;
        out.push((x.round() as i32, y.round() as i32));
    }
    dedupe_consecutive_points(out)
}

fn dedupe_consecutive_points(points: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    let mut out = Vec::with_capacity(points.len());
    for p in points {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_by_point_count_returns_empty_for_same_point() {
        let p = plan_linear_by_point_count((10, 10), (10, 10), 10);
        assert!(p.is_empty());
    }

    #[test]
    fn linear_by_point_count_default_has_ten_waypoints() {
        let p = plan_linear_by_point_count((0, 0), (100, 0), MOUSE_MOVE_DEFAULT_POINT_COUNT);
        assert_eq!(p.len(), MOUSE_MOVE_DEFAULT_POINT_COUNT);
        assert_eq!(p.last().copied(), Some((100, 0)));
    }

    #[test]
    fn linear_by_point_count_ends_at_target() {
        let p = plan_linear_by_point_count((0, 0), (100, 0), 10);
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
        assert_eq!(plan.path.points.len(), MOUSE_MOVE_DEFAULT_POINT_COUNT);
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
    fn ease_in_out_middle_larger_than_ends() {
        let intervals = ease_in_out_intervals(5, 1.0);
        assert!(intervals[2] > intervals[0]);
        assert!(intervals[2] > intervals[4]);
    }

    #[test]
    fn execute_move_plan_calls_all_points_and_final_perception() {
        let plan = MouseMovePlan {
            path: MouseMovePath {
                points: vec![(1, 1), (2, 2), (3, 3)],
            },
            timing: MouseMoveTimingPlan {
                step_intervals_secs: vec![0.0, 0.0, 0.0],
            },
        };
        let mut seen = Vec::new();
        execute_move_plan(&plan, (3, 3), |x, y| -> Result<(), ()> {
            seen.push((x, y));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, vec![(1, 1), (2, 2), (3, 3), (3, 3)]);
    }

    #[test]
    fn execute_move_plan_empty_path_still_posts_target() {
        let plan = MouseMovePlan {
            path: MouseMovePath { points: vec![] },
            timing: MouseMoveTimingPlan {
                step_intervals_secs: vec![],
            },
        };
        let mut seen = Vec::new();
        execute_move_plan(&plan, (9, 9), |x, y| -> Result<(), ()> {
            seen.push((x, y));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, vec![(9, 9)]);
    }
}
