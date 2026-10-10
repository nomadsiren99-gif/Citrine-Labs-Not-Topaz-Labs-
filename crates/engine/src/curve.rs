//! Curve tool v1 (spec Milestone 7). Monotone cubic (Fritsch-Butland) through the user's
//! points, baked to a 1024-entry LUT. Applied to luminance, so hue is preserved. Independent
//! proposal, not a reconstruction of Topaz.

use crate::adaptive_exposure::luma;
use crate::buffer::{EngineError, LinearImage};

const LUT_SIZE: usize = 1024;
const LUMA_EPS: f32 = 1e-6;

/// Tone curve through points in 0..1. Neutral is the identity line, `[(0,0), (1,1)]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub points: Vec<(f32, f32)>,
}

impl Default for Curve {
    fn default() -> Self {
        Self { points: vec![(0.0, 0.0), (1.0, 1.0)] }
    }
}

impl Curve {
    /// Applies in place. Points are clamped to 0..1 and sorted; duplicate x values are dropped.
    /// Fewer than two usable points is an error.
    pub fn apply(&self, img: &mut LinearImage) -> Result<(), EngineError> {
        if self.points.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
            return Err(EngineError::NonFiniteParameter("curve point"));
        }
        let pts = clean(&self.points);
        if pts.len() < 2 {
            return Err(EngineError::InvalidParameter("curve needs two distinct points"));
        }
        if pts == [(0.0, 0.0), (1.0, 1.0)] {
            return Ok(());
        }
        let lut = bake(&pts);
        for px in img.data_mut().chunks_exact_mut(4) {
            let y = luma(px);
            if !y.is_finite() || y <= LUMA_EPS {
                continue;
            }
            let t = y.clamp(0.0, 1.0) * (LUT_SIZE - 1) as f32;
            let i = t.floor() as usize;
            let f = t - i as f32;
            let next = lut[(i + 1).min(LUT_SIZE - 1)];
            let mapped = lut[i] + f * (next - lut[i]);
            let g = mapped / y;
            px[0] *= g;
            px[1] *= g;
            px[2] *= g;
        }
        Ok(())
    }
}

fn clean(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut p: Vec<(f32, f32)> = points.iter().map(|(x, y)| (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))).collect();
    p.sort_by(|a, b| a.0.total_cmp(&b.0));
    p.dedup_by(|a, b| a.0 == b.0);
    p
}

/// Monotone cubic through sorted points, sampled over the LUT domain.
fn bake(p: &[(f32, f32)]) -> Vec<f32> {
    let n = p.len();
    let h: Vec<f32> = (0..n - 1).map(|i| p[i + 1].0 - p[i].0).collect();
    let d: Vec<f32> = (0..n - 1).map(|i| (p[i + 1].1 - p[i].1) / h[i]).collect();
    let mut m = vec![0.0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        if d[i - 1] * d[i] <= 0.0 {
            m[i] = 0.0;
        } else {
            let w1 = 2.0 * h[i] + h[i - 1];
            let w2 = h[i] + 2.0 * h[i - 1];
            m[i] = (w1 + w2) / (w1 / d[i - 1] + w2 / d[i]);
        }
    }
    (0..LUT_SIZE)
        .map(|k| {
            let x = k as f32 / (LUT_SIZE - 1) as f32;
            let seg = match p.iter().position(|q| q.0 > x) {
                Some(0) => 0,
                Some(j) => j - 1,
                None => n - 2,
            }
            .min(n - 2);
            let t = (x - p[seg].0) / h[seg];
            let (y0, y1) = (p[seg].1, p[seg + 1].1);
            let (m0, m1) = (m[seg] * h[seg], m[seg + 1] * h[seg]);
            let t2 = t * t;
            let t3 = t2 * t;
            (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * m0 + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * m1
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> LinearImage {
        let mut rgba = Vec::new();
        for v in (0..=255).step_by(5) {
            rgba.extend_from_slice(&[v as u8, v as u8, v as u8, 255]);
        }
        LinearImage::from_srgb8(rgba.len() / 4, 1, &rgba).unwrap()
    }

    #[test]
    fn identity_curve_is_bit_identical() {
        let mut img = ramp();
        let before = img.clone();
        Curve::default().apply(&mut img).unwrap();
        assert_eq!(img, before);
    }

    #[test]
    fn raising_the_midpoint_brightens_midtones() {
        let before = ramp();
        let mut img = ramp();
        Curve { points: vec![(0.0, 0.0), (0.5, 0.7), (1.0, 1.0)] }.apply(&mut img).unwrap();
        let mid = before.width() / 2;
        assert!(img.data()[mid * 4] > before.data()[mid * 4]);
    }

    #[test]
    fn monotone_points_give_a_monotone_curve_without_overshoot() {
        let lut = bake(&[(0.0, 0.0), (0.2, 0.05), (0.5, 0.6), (0.8, 0.62), (1.0, 1.0)]);
        assert!(lut.windows(2).all(|w| w[1] >= w[0] - 1e-6), "not monotone");
        assert!(lut.iter().all(|v| *v >= -1e-6 && *v <= 1.0 + 1e-6), "overshoot");
    }

    #[test]
    fn hue_is_preserved() {
        let mut rgba = Vec::new();
        for _ in 0..8 {
            rgba.extend_from_slice(&[200, 100, 50, 255]);
        }
        let mut img = LinearImage::from_srgb8(8, 1, &rgba).unwrap();
        let before = img.clone();
        Curve { points: vec![(0.0, 0.0), (0.5, 0.8), (1.0, 1.0)] }.apply(&mut img).unwrap();
        let (a, b) = (&before.data()[0..3], &img.data()[0..3]);
        assert!((a[0] / a[2] - b[0] / b[2]).abs() < 1e-4);
    }

    #[test]
    fn rejects_non_finite_and_too_few_points() {
        let mut img = ramp();
        assert!(Curve { points: vec![(0.0, f32::NAN), (1.0, 1.0)] }.apply(&mut img).is_err());
        assert!(Curve { points: vec![(0.5, 0.5), (0.5, 0.9)] }.apply(&mut img).is_err());
    }
}
