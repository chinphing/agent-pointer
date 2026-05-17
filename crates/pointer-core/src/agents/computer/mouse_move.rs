//! Mouse movement planning/execution abstraction for computer agent backends.
//!
//! Geometry (waypoints) and timing (per-step delays) are planned separately, matching
//! the Python `MouseMove._generate_path` / `_calculate_intervals` split.
//!
//! Default path: straight line at 14px steps, then 5px re-sample on the final segment; the
//! last waypoint before the target is 1px away. Default timing: 0.5s total, ease-out. See
//! `docs/design/computer-mouse-movement-roadmap.md`.

use super::timing::{
    MOUSE_MOVE_APPROACH_FINAL_GAP_PX, MOUSE_MOVE_APPROACH_STEP_MAX_PX,
    MOUSE_MOVE_LINEAR_STEP_MAX_PX, MOUSE_MOVE_TOTAL_DURATION_SECS,
};
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
    /// Straight line with uniform spatial stepping.
    LinearUniform,
}

/// Tunables for path (geometry) planning.
#[derive(Debug, Clone, Copy)]
pub struct MouseMovePathConfig {
    pub strategy: MouseMoveStrategy,
    /// Maximum pixel distance between consecutive points.
    pub max_step_px: f64,
    /// Re-sample the final segment at this spacing (px) for hover hit-testing near the target.
    pub approach_max_step_px: f64,
}

impl Default for MouseMovePathConfig {
    fn default() -> Self {
        Self {
            strategy: MouseMoveStrategy::LinearUniform,
            max_step_px: MOUSE_MOVE_LINEAR_STEP_MAX_PX,
            approach_max_step_px: MOUSE_MOVE_APPROACH_STEP_MAX_PX,
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
            MouseMoveStrategy::LinearUniform => MouseMovePath {
                points: plan_linear_uniform_with_dense_approach(
                    from,
                    to,
                    self.config.max_step_px,
                    self.config.approach_max_step_px,
                ),
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

/// Coarse linear path, then 5px re-sample on the final segment toward the target.
fn plan_linear_uniform_with_dense_approach(
    from: (i32, i32),
    to: (i32, i32),
    max_step_px: f64,
    approach_max_step_px: f64,
) -> Vec<(i32, i32)> {
    let mut path = plan_linear_uniform_path(from, to, max_step_px);
    if path.is_empty() {
        return path;
    }

    let approach_start = if path.len() >= 2 {
        path[path.len() - 2]
    } else {
        from
    };
    let target = path
        .last()
        .copied()
        .expect("non-empty path has a last point");

    let dense_tail = plan_approach_segment(
        approach_start,
        target,
        approach_max_step_px,
        MOUSE_MOVE_APPROACH_FINAL_GAP_PX,
    );

    if path.len() >= 2 {
        path.truncate(path.len() - 2);
    } else {
        path.clear();
    }
    path.extend(dense_tail);
    path
}

/// Dense steps toward `target`, then a 1px-from-target waypoint, then `target`.
fn plan_approach_segment(
    start: (i32, i32),
    target: (i32, i32),
    step_px: f64,
    final_gap_px: f64,
) -> Vec<(i32, i32)> {
    if start == target {
        return Vec::new();
    }

    let dx = (target.0 - start.0) as f64;
    let dy = (target.1 - start.1) as f64;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < 1e-6 {
        return Vec::new();
    }

    let ux = dx / distance;
    let uy = dy / distance;
    let gap_px = if final_gap_px.is_finite() && final_gap_px > 0.0 {
        final_gap_px.min(distance - 1e-6)
    } else {
        MOUSE_MOVE_APPROACH_FINAL_GAP_PX.min(distance - 1e-6)
    };

    let gap = (
        (target.0 as f64 - ux * gap_px).round() as i32,
        (target.1 as f64 - uy * gap_px).round() as i32,
    );

    if gap == target {
        return vec![target];
    }

    let mut out = if gap == start {
        Vec::new()
    } else {
        plan_linear_uniform_path(start, gap, step_px)
    };

    if out.last() != Some(&gap) {
        out.push(gap);
    }
    if out.last() != Some(&target) {
        out.push(target);
    }
    out
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
    fn dense_approach_adds_points_on_final_segment() {
        let coarse = plan_linear_uniform_path((0, 0), (100, 0), 14.0);
        let dense = plan_linear_uniform_with_dense_approach((0, 0), (100, 0), 14.0, 5.0);
        assert!(dense.len() > coarse.len());
        assert_eq!(dense.last().copied(), Some((100, 0)));
    }

    #[test]
    fn dense_approach_short_move_ends_one_px_before_target() {
        let p = plan_linear_uniform_with_dense_approach((0, 0), (12, 0), 14.0, 5.0);
        assert_eq!(p.last().copied(), Some((12, 0)));
        assert_eq!(p[p.len() - 2], (11, 0));
    }

    #[test]
    fn approach_segment_penultimate_is_one_px_from_target() {
        let p = plan_approach_segment((0, 0), (20, 0), 5.0, 1.0);
        assert_eq!(p.last().copied(), Some((20, 0)));
        let pen = p[p.len() - 2];
        let dist =
            (((20 - pen.0).pow(2) + (0 - pen.1).pow(2)) as f64).sqrt();
        assert!((dist - 1.0).abs() < 1e-6, "penultimate {pen:?} dist {dist}");
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
