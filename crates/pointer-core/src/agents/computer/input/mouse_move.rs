//! Mouse movement planning/execution abstraction for computer agent backends.
//!
//! Geometry (waypoints) and timing (per-step delays) are planned separately, matching
//! the Python `MouseMove._generate_path` / `_calculate_intervals` split.
//!
//! See `docs/design/computer-mouse-movement-roadmap.md`.

use super::mouse_path::{bezier_path, BezierPathConfig, DEFAULT_CONTROL_JITTER_PX};
use super::timing::{
    MOUSE_MOVE_DEFAULT_POINT_COUNT, MOUSE_MOVE_TOTAL_DURATION_MAX_SECS,
    MOUSE_MOVE_TOTAL_DURATION_MIN_SECS, MOUSE_MOVE_TOTAL_DURATION_SECS,
};
use log::debug;
use rand::Rng;
use std::time::Duration;

/// Default pre-move jitter radius in pixels (Python `DEFAULT_JITTER_RADIUS_PX`).
pub const DEFAULT_PRE_JITTER_RADIUS_PX: i32 = 10;
pub const DEFAULT_PRE_JITTER_SLEEP_MIN_SECS: f64 = 0.2;
pub const DEFAULT_PRE_JITTER_SLEEP_MAX_SECS: f64 = 0.5;
pub const DEFAULT_PATH_JITTER_MAX_PX: f64 = 2.0;
pub const DEFAULT_INTERVAL_PERTURB_FACTOR: f64 = 0.2;

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

/// How total duration is interpreted (Python `MoveOptions.duration_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationMode {
    /// Fixed interval per step.
    Step,
    /// Intervals sum to `total_duration_secs` with easing.
    Total,
    /// Like Total, then random per-interval perturbation preserving sum.
    TotalPerturb,
}

/// Planned movement strategy kind (geometry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMoveStrategy {
    /// Straight line with `point_count` uniformly spaced waypoints.
    LinearByPointCount,
    /// Cubic Bézier (~10 points), aligned with Python `mouse_path_spline`.
    Bezier,
}

/// Tunables for path (geometry) planning.
#[derive(Debug, Clone, Copy)]
pub struct MouseMovePathConfig {
    pub strategy: MouseMoveStrategy,
    pub point_count: usize,
    pub bezier: BezierPathConfig,
}

impl Default for MouseMovePathConfig {
    fn default() -> Self {
        Self {
            strategy: MouseMoveStrategy::LinearByPointCount,
            point_count: MOUSE_MOVE_DEFAULT_POINT_COUNT,
            bezier: BezierPathConfig::default(),
        }
    }
}

/// Tunables for timing (interval) planning.
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveTimingConfig {
    pub duration_mode: DurationMode,
    /// Used when [`Self::use_random_total_duration`] is false.
    pub total_duration_secs: f64,
    /// When true, each move samples [`Self::total_duration_min_secs`]..=[`Self::total_duration_max_secs`].
    pub use_random_total_duration: bool,
    pub total_duration_min_secs: f64,
    pub total_duration_max_secs: f64,
    pub step_duration_secs: f64,
    pub ease_in_out: bool,
    pub interval_perturb_factor: f64,
}

impl Default for MouseMoveTimingConfig {
    fn default() -> Self {
        Self {
            duration_mode: DurationMode::Total,
            total_duration_secs: MOUSE_MOVE_TOTAL_DURATION_SECS,
            use_random_total_duration: true,
            total_duration_min_secs: MOUSE_MOVE_TOTAL_DURATION_MIN_SECS,
            total_duration_max_secs: MOUSE_MOVE_TOTAL_DURATION_MAX_SECS,
            step_duration_secs: 0.03,
            ease_in_out: false,
            interval_perturb_factor: DEFAULT_INTERVAL_PERTURB_FACTOR,
        }
    }
}

/// Pre/post execution flags (Python `MoveOptions` jitter/delay fields).
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveExecConfig {
    pub pre_delay_secs: f64,
    pub post_delay_secs: f64,
    pub pre_jitter: bool,
    pub pre_jitter_steps: u32,
    pub pre_jitter_radius_px: i32,
    pub pre_jitter_sleep_min_secs: f64,
    pub pre_jitter_sleep_max_secs: f64,
    pub path_jitter: bool,
    pub path_jitter_max_px: f64,
}

impl Default for MouseMoveExecConfig {
    fn default() -> Self {
        Self {
            pre_delay_secs: 0.0,
            post_delay_secs: 0.0,
            pre_jitter: false,
            pre_jitter_steps: 2,
            pre_jitter_radius_px: DEFAULT_PRE_JITTER_RADIUS_PX,
            pre_jitter_sleep_min_secs: DEFAULT_PRE_JITTER_SLEEP_MIN_SECS,
            pre_jitter_sleep_max_secs: DEFAULT_PRE_JITTER_SLEEP_MAX_SECS,
            path_jitter: false,
            path_jitter_max_px: DEFAULT_PATH_JITTER_MAX_PX,
        }
    }
}

/// Full movement profile: path + timing + execution (Python `MoveOptions` + `MouseMove` config).
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveProfile {
    pub path: MouseMovePathConfig,
    pub timing: MouseMoveTimingConfig,
    pub exec: MouseMoveExecConfig,
}

impl MouseMoveProfile {
    /// Default when `human_like` is off: straight line, 10 waypoints, 0.5–1.5s ease-out (no jitter).
    pub fn standard() -> Self {
        Self {
            path: MouseMovePathConfig::default(),
            timing: MouseMoveTimingConfig::default(),
            exec: MouseMoveExecConfig::default(),
        }
    }

    /// Human-like move (Python `MouseHelper.move_to_position(human_like=True)`).
    pub fn human_like() -> Self {
        Self {
            path: MouseMovePathConfig {
                strategy: MouseMoveStrategy::Bezier,
                point_count: MOUSE_MOVE_DEFAULT_POINT_COUNT,
                bezier: BezierPathConfig {
                    num_points: MOUSE_MOVE_DEFAULT_POINT_COUNT,
                    curvature: 1.0,
                    bend_sign: None,
                    bend_pixels: None,
                    control_jitter_px: Some(DEFAULT_CONTROL_JITTER_PX),
                },
            },
            timing: MouseMoveTimingConfig {
                duration_mode: DurationMode::Total,
                ease_in_out: false,
                ..Default::default()
            },
            exec: MouseMoveExecConfig {
                pre_jitter: true,
                pre_jitter_steps: 2,
                path_jitter: true,
                ..Default::default()
            },
        }
    }

    /// Drag: move to start (Python `drag_from_to` first segment).
    pub fn drag_to_start() -> Self {
        Self {
            path: MouseMovePathConfig {
                strategy: MouseMoveStrategy::Bezier,
                point_count: MOUSE_MOVE_DEFAULT_POINT_COUNT,
                bezier: BezierPathConfig::default(),
            },
            timing: MouseMoveTimingConfig {
                duration_mode: DurationMode::Total,
                total_duration_secs: 0.35,
                use_random_total_duration: false,
                ease_in_out: true,
                ..Default::default()
            },
            exec: MouseMoveExecConfig {
                pre_delay_secs: 0.05,
                ..Default::default()
            },
        }
    }

    /// Drag: segment while button held.
    pub fn drag_segment(human_like: bool) -> Self {
        if human_like {
            Self {
                path: MouseMovePathConfig {
                    strategy: MouseMoveStrategy::Bezier,
                    point_count: MOUSE_MOVE_DEFAULT_POINT_COUNT,
                    bezier: BezierPathConfig::default(),
                },
                timing: MouseMoveTimingConfig {
                    duration_mode: DurationMode::TotalPerturb,
                    total_duration_secs: 0.45,
                    use_random_total_duration: false,
                    ease_in_out: true,
                    interval_perturb_factor: 0.15,
                    ..Default::default()
                },
                exec: MouseMoveExecConfig::default(),
            }
        } else {
            Self {
                path: MouseMovePathConfig {
                    strategy: MouseMoveStrategy::LinearByPointCount,
                    point_count: MOUSE_MOVE_DEFAULT_POINT_COUNT,
                    bezier: BezierPathConfig::default(),
                },
                timing: MouseMoveTimingConfig {
                    duration_mode: DurationMode::Total,
                    total_duration_secs: 0.18,
                    use_random_total_duration: false,
                    ease_in_out: false,
                    ..Default::default()
                },
                exec: MouseMoveExecConfig::default(),
            }
        }
    }
}

impl Default for MouseMoveProfile {
    fn default() -> Self {
        Self::standard()
    }
}

/// Plans cursor geometry (waypoints).
pub struct MouseMovePathPlanner {
    config: MouseMovePathConfig,
}

impl MouseMovePathPlanner {
    pub fn new(config: MouseMovePathConfig) -> Self {
        Self { config }
    }

    pub fn plan(&self, from: (i32, i32), to: (i32, i32), rng: &mut impl Rng) -> MouseMovePath {
        let points = match self.config.strategy {
            MouseMoveStrategy::LinearByPointCount => {
                plan_linear_by_point_count(from, to, self.config.point_count)
            }
            MouseMoveStrategy::Bezier => {
                let from_f = (from.0 as f64, from.1 as f64);
                let to_f = (to.0 as f64, to.1 as f64);
                if from == to {
                    Vec::new()
                } else {
                    bezier_path(from_f, to_f, self.config.bezier, rng)
                }
            }
        };
        MouseMovePath { points }
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

    pub fn plan(&self, num_steps: usize, rng: &mut impl Rng) -> MouseMoveTimingPlan {
        let total_secs = if self.config.use_random_total_duration {
            rng.gen_range(self.config.total_duration_min_secs..=self.config.total_duration_max_secs)
        } else {
            self.config.total_duration_secs
        };
        let mut step_intervals_secs = match self.config.duration_mode {
            DurationMode::Step => vec![self.config.step_duration_secs; num_steps],
            DurationMode::Total | DurationMode::TotalPerturb => {
                if self.config.ease_in_out {
                    ease_in_out_intervals(num_steps, total_secs)
                } else {
                    ease_out_intervals(num_steps, total_secs)
                }
            }
        };
        if self.config.duration_mode == DurationMode::TotalPerturb && num_steps > 0 {
            step_intervals_secs = spread_interval_perturbation(
                &step_intervals_secs,
                self.config.interval_perturb_factor,
                rng,
            );
        }
        MouseMoveTimingPlan {
            step_intervals_secs,
        }
    }
}

/// Composes path and timing planners (Python `MouseMove` equivalent).
pub struct MouseMovePlanner {
    path: MouseMovePathPlanner,
    timing: MouseMoveTimingPlanner,
    exec: MouseMoveExecConfig,
}

impl MouseMovePlanner {
    pub fn from_profile(profile: MouseMoveProfile) -> Self {
        Self {
            path: MouseMovePathPlanner::new(profile.path),
            timing: MouseMoveTimingPlanner::new(profile.timing),
            exec: profile.exec,
        }
    }

    pub fn plan(&self, from: (i32, i32), to: (i32, i32), rng: &mut impl Rng) -> MouseMovePlan {
        let mut path = self.path.plan(from, to, rng);
        if self.exec.path_jitter && !path.points.is_empty() {
            path.points = add_path_jitter(&path.points, self.exec.path_jitter_max_px, rng);
        }
        let timing = self.timing.plan(path.points.len(), rng);
        MouseMovePlan { path, timing }
    }
}

/// Execute a movement plan with optional pre-jitter / delays.
pub fn execute_move_plan<E>(
    plan: &MouseMovePlan,
    from: (i32, i32),
    target: (i32, i32),
    exec: &MouseMoveExecConfig,
    mut move_abs: impl FnMut(i32, i32) -> Result<(), E>,
    rng: &mut impl Rng,
) -> Result<(), E> {
    if exec.pre_delay_secs > 0.0 {
        std::thread::sleep(Duration::from_secs_f64(exec.pre_delay_secs));
    }
    if exec.pre_jitter {
        pre_jitter_near_cursor_from(
            from,
            exec.pre_jitter_radius_px,
            exec.pre_jitter_steps,
            exec.pre_jitter_sleep_min_secs,
            exec.pre_jitter_sleep_max_secs,
            &mut move_abs,
            rng,
        )?;
    }

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

    if exec.post_delay_secs > 0.0 {
        std::thread::sleep(Duration::from_secs_f64(exec.post_delay_secs));
    }
    Ok(())
}

/// Random normal-offset jitter on interior path points (Python `_add_path_jitter`).
pub fn add_path_jitter(points: &[(i32, i32)], max_px: f64, rng: &mut impl Rng) -> Vec<(i32, i32)> {
    if points.is_empty() || max_px <= 0.0 {
        return points.to_vec();
    }
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (x, y) = points[i];
        if i == 0 || i == n - 1 {
            out.push((x, y));
            continue;
        }
        let (dx, dy) = if i == 0 {
            (points[1].0 as f64 - x as f64, points[1].1 as f64 - y as f64)
        } else if i == n - 1 {
            (
                x as f64 - points[i - 1].0 as f64,
                y as f64 - points[i - 1].1 as f64,
            )
        } else {
            (
                (points[i + 1].0 - points[i - 1].0) as f64 / 2.0,
                (points[i + 1].1 - points[i - 1].1) as f64 / 2.0,
            )
        };
        let length = (dx * dx + dy * dy).sqrt();
        if length < 1e-6 {
            out.push((x, y));
            continue;
        }
        let mut nx = -dy / length;
        let mut ny = dx / length;
        if rng.gen_bool(0.5) {
            nx = -nx;
            ny = -ny;
        }
        let jitter = max_px * (2.0 * rng.gen::<f64>() - 1.0);
        out.push((
            (x as f64 + nx * jitter).round() as i32,
            (y as f64 + ny * jitter).round() as i32,
        ));
    }
    dedupe_consecutive_points(out)
}

/// Random multiplicative perturbation; total sum preserved (Python `_spread_interval_perturbation`).
pub fn spread_interval_perturbation(
    intervals: &[f64],
    factor: f64,
    rng: &mut impl Rng,
) -> Vec<f64> {
    if intervals.is_empty() || factor <= 0.0 {
        return intervals.to_vec();
    }
    let total: f64 = intervals.iter().sum();
    let perturbed: Vec<f64> = intervals
        .iter()
        .map(|&iv| {
            let scale = 1.0 + factor * (2.0 * rng.gen::<f64>() - 1.0);
            (iv * scale).max(0.005)
        })
        .collect();
    let sum_p: f64 = perturbed.iter().sum();
    if sum_p <= 0.0 {
        return intervals.to_vec();
    }
    let scale = total / sum_p;
    perturbed.iter().map(|p| p * scale).collect()
}

/// Pre-jitter from a known cursor position (Python `_mouse_jitter_near_cursor`).
pub fn pre_jitter_near_cursor_from<E>(
    current: (i32, i32),
    jitter_radius_px: i32,
    steps: u32,
    sleep_min: f64,
    sleep_max: f64,
    mut move_abs: impl FnMut(i32, i32) -> Result<(), E>,
    rng: &mut impl Rng,
) -> Result<(), E> {
    let radius = jitter_radius_px.max(1);
    for _ in 0..steps.max(1) {
        let dx: i32 = rng.gen_range(-radius..=radius);
        let dy: i32 = rng.gen_range(-radius..=radius);
        let cx = current.0 + dx;
        let cy = current.1 + dy;
        move_abs(cx, cy)?;
        let sleep_secs = if sleep_max > sleep_min {
            sleep_min + rng.gen::<f64>() * (sleep_max - sleep_min)
        } else {
            sleep_min
        };
        if sleep_secs > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(sleep_secs));
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
    use rand::rngs::StdRng;
    use rand::SeedableRng;

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
    fn human_like_profile_plans_bezier_points() {
        let mut rng = StdRng::seed_from_u64(99);
        let planner = MouseMovePlanner::from_profile(MouseMoveProfile::human_like());
        let plan = planner.plan((0, 0), (300, 0), &mut rng);
        assert_eq!(plan.path.points.len(), MOUSE_MOVE_DEFAULT_POINT_COUNT);
    }

    #[test]
    fn standard_profile_ten_linear_waypoints() {
        let mut rng = StdRng::seed_from_u64(1);
        let planner = MouseMovePlanner::from_profile(MouseMoveProfile::standard());
        let plan = planner.plan((0, 0), (100, 0), &mut rng);
        assert_eq!(plan.path.points.len(), MOUSE_MOVE_DEFAULT_POINT_COUNT);
        let sum: f64 = plan.timing.step_intervals_secs.iter().sum();
        assert!(sum >= MOUSE_MOVE_TOTAL_DURATION_MIN_SECS - 1e-6);
        assert!(sum <= MOUSE_MOVE_TOTAL_DURATION_MAX_SECS + 1e-6);
    }

    #[test]
    fn ease_out_intervals_sum_to_total() {
        let intervals = ease_out_intervals(10, 0.5);
        assert_eq!(intervals.len(), 10);
        let sum: f64 = intervals.iter().sum();
        assert!((sum - 0.5).abs() < 1e-6);
    }

    #[test]
    fn spread_interval_perturbation_preserves_sum() {
        let mut rng = StdRng::seed_from_u64(42);
        let base = ease_out_intervals(8, 0.45);
        let perturbed = spread_interval_perturbation(&base, 0.15, &mut rng);
        let sum: f64 = perturbed.iter().sum();
        assert!((sum - 0.45).abs() < 1e-6);
    }

    #[test]
    fn add_path_jitter_keeps_endpoints() {
        let mut rng = StdRng::seed_from_u64(5);
        let points = vec![(0, 0), (50, 10), (100, 0)];
        let out = add_path_jitter(&points, 3.0, &mut rng);
        assert_eq!(out.first(), Some(&(0, 0)));
        assert_eq!(out.last(), Some(&(100, 0)));
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
        let mut rng = StdRng::seed_from_u64(0);
        execute_move_plan(
            &plan,
            (0, 0),
            (3, 3),
            &MouseMoveExecConfig::default(),
            |x, y| -> Result<(), ()> {
                seen.push((x, y));
                Ok(())
            },
            &mut rng,
        )
        .unwrap();
        assert_eq!(seen, vec![(1, 1), (2, 2), (3, 3), (3, 3)]);
    }
}
