//! Cubic Bézier mouse trajectories (Python `mouse_path.py` parity, no scipy).

use rand::Rng;

const CURVATURE_BASE_RATIO: f64 = 0.12;
const MIN_BEND_PX: f64 = 5.0;
const MAX_BEND_PX: f64 = 60.0;
pub const DEFAULT_CONTROL_JITTER_PX: f64 = 2.0;

/// Geometry tunables for one segment (Python `mouse_path` / `mouse_path_spline` args).
#[derive(Debug, Clone, Copy)]
pub struct BezierPathConfig {
    pub num_points: usize,
    pub curvature: f64,
    /// `None` = random ±1 per segment; `Some(1)` / `Some(-1)` fixes bend side.
    pub bend_sign: Option<i32>,
    pub bend_pixels: Option<f64>,
    pub control_jitter_px: Option<f64>,
}

impl Default for BezierPathConfig {
    fn default() -> Self {
        Self {
            num_points: 10,
            curvature: 1.0,
            bend_sign: None,
            bend_pixels: None,
            control_jitter_px: Some(DEFAULT_CONTROL_JITTER_PX),
        }
    }
}

/// Sample a cubic Bézier from `from` to `to` (excludes start, includes end).
pub fn bezier_path(
    from: (f64, f64),
    to: (f64, f64),
    config: BezierPathConfig,
    rng: &mut impl Rng,
) -> Vec<(i32, i32)> {
    let num_points = config.num_points.max(1);
    let (p0, p1, p2, p3) = segment_control_points(from, to, config, rng);
    let samples = bezier_segment(p0, p1, p2, p3, num_points);
    quantize_path(samples)
}

fn segment_control_points(
    from: (f64, f64),
    to: (f64, f64),
    config: BezierPathConfig,
    rng: &mut impl Rng,
) -> ((f64, f64), (f64, f64), (f64, f64), (f64, f64)) {
    let (x0, y0) = from;
    let (x1, y1) = to;
    let diff_x = x1 - x0;
    let diff_y = y1 - y0;
    let l = (diff_x * diff_x + diff_y * diff_y).sqrt();
    let p0 = (x0, y0);
    let p3 = (x1, y1);
    if l < 1e-6 {
        return (p0, p0, p3, p3);
    }
    let nx = -diff_y / l;
    let ny = diff_x / l;
    let tx = diff_x / l;
    let ty = diff_y / l;

    let bend = if let Some(bp) = config.bend_pixels {
        bp
    } else if config.curvature <= 0.0 {
        0.0
    } else {
        let base = (CURVATURE_BASE_RATIO * l).clamp(MIN_BEND_PX, MAX_BEND_PX);
        let mag = config.curvature * base;
        let sign_f = match config.bend_sign {
            Some(s) if s > 0 => 1.0,
            Some(_) => -1.0,
            None => {
                if rng.gen_bool(0.5) {
                    1.0
                } else {
                    -1.0
                }
            }
        };
        sign_f * mag
    };

    let mut p1x = x0 + diff_x / 3.0 + nx * bend;
    let mut p1y = y0 + diff_y / 3.0 + ny * bend;
    let mut p2x = x1 - diff_x / 3.0 + nx * bend;
    let mut p2y = y1 - diff_y / 3.0 + ny * bend;

    let j = config
        .control_jitter_px
        .unwrap_or(DEFAULT_CONTROL_JITTER_PX);
    if j > 0.0 {
        let a1 = j * (2.0 * rng.gen::<f64>() - 1.0);
        let b1 = j * (2.0 * rng.gen::<f64>() - 1.0);
        let a2 = j * (2.0 * rng.gen::<f64>() - 1.0);
        let b2 = j * (2.0 * rng.gen::<f64>() - 1.0);
        p1x += tx * a1 + nx * b1;
        p1y += ty * a1 + ny * b1;
        p2x += tx * a2 + nx * b2;
        p2y += ty * a2 + ny * b2;
    }

    (p0, (p1x, p1y), (p2x, p2y), p3)
}

fn bezier_segment(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    num_points: usize,
) -> Vec<(f64, f64)> {
    let n = num_points.max(1);
    let mut out = Vec::with_capacity(n);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let one_t = 1.0 - t;
        let x = one_t.powi(3) * p0.0
            + 3.0 * one_t.powi(2) * t * p1.0
            + 3.0 * one_t * t.powi(2) * p2.0
            + t.powi(3) * p3.0;
        let y = one_t.powi(3) * p0.1
            + 3.0 * one_t.powi(2) * t * p1.1
            + 3.0 * one_t * t.powi(2) * p2.1
            + t.powi(3) * p3.1;
        out.push((x, y));
    }
    out
}

fn quantize_path(points: Vec<(f64, f64)>) -> Vec<(i32, i32)> {
    let mut out = Vec::with_capacity(points.len());
    for (x, y) in points {
        let p = (x.round() as i32, y.round() as i32);
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
    fn bezier_ends_at_target() {
        let mut rng = StdRng::seed_from_u64(42);
        let cfg = BezierPathConfig {
            bend_sign: Some(1),
            control_jitter_px: Some(0.0),
            ..Default::default()
        };
        let path = bezier_path((0.0, 0.0), (100.0, 0.0), cfg, &mut rng);
        assert_eq!(path.len(), 10);
        assert_eq!(path.last().copied(), Some((100, 0)));
    }

    #[test]
    fn bezier_midpoint_deviates_from_straight_line() {
        let mut rng = StdRng::seed_from_u64(7);
        let cfg = BezierPathConfig {
            bend_sign: Some(1),
            control_jitter_px: Some(0.0),
            curvature: 1.0,
            num_points: 10,
            bend_pixels: Some(30.0),
        };
        let path = bezier_path((0.0, 0.0), (200.0, 0.0), cfg, &mut rng);
        let mid = path[path.len() / 2];
        assert!(mid.1.abs() > 2, "expected visible bend, got {:?}", mid);
    }

    #[test]
    fn bezier_same_point_empty() {
        let mut rng = StdRng::seed_from_u64(1);
        let path = bezier_path(
            (5.0, 5.0),
            (5.0, 5.0),
            BezierPathConfig::default(),
            &mut rng,
        );
        assert!(path.is_empty() || path == vec![(5, 5)]);
    }
}
