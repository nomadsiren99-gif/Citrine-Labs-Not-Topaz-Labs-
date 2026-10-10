//! Adaptive exposure v1 (spec Milestone 3). Independent proposal, not a reconstruction of
//! Topaz's algorithm. Each pixel is lifted toward the image's global mean log-luminance,
//! measured against its own local mean. Mappings below are proposed, not verified.

use crate::buffer::{EngineError, LinearImage};
use crate::exposure::{apply_exposure, MAX_EV};

/// Largest per-pixel lift before the user gain, in stops. Proposed limit.
pub const MAX_LIFT_EV: f32 = 3.0;
/// Floor for log2 so black pixels stay finite.
const LUMA_EPS: f32 = 1e-6;
/// Regions are clamped to this range. Proposed.
const MAX_REGIONS: u32 = 64;

/// Parameters for adaptive exposure. `Default` is neutral: the image is returned unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveExposure {
    /// 0 = off, 1 = full. Scales the lift.
    pub strength: f32,
    /// Subdivisions along the longest side. Blur radius = longest side / regions / 2.
    pub regions: u32,
    /// -1..1. Multiplies the lift by (1 + contrast). Proposed mapping.
    pub contrast: f32,
    /// Global offset in EV, added after the lift.
    pub brightness_ev: f32,
    /// 0..1. Attenuates positive lift on highlights (luminance >= 0.5).
    pub protect_highlights: f32,
    /// 0..1. Attenuates negative lift in shadows (luminance < 0.25).
    pub protect_shadows: f32,
}

impl Default for AdaptiveExposure {
    fn default() -> Self {
        Self {
            strength: 0.0,
            regions: 4,
            contrast: 0.0,
            brightness_ev: 0.0,
            protect_highlights: 0.0,
            protect_shadows: 0.0,
        }
    }
}

impl AdaptiveExposure {
    /// Applies the adjustment in place. Out-of-range values are clamped; non-finite values are
    /// rejected before any pixel changes.
    pub fn apply(&self, img: &mut LinearImage) -> Result<(), EngineError> {
        for (name, v) in [
            ("strength", self.strength),
            ("contrast", self.contrast),
            ("brightness_ev", self.brightness_ev),
            ("protect_highlights", self.protect_highlights),
            ("protect_shadows", self.protect_shadows),
        ] {
            if !v.is_finite() {
                return Err(EngineError::NonFiniteParameter(name));
            }
        }

        let strength = self.strength.clamp(0.0, 1.0);
        if strength == 0.0 {
            // Lift is zero, so only the global offset remains.
            return apply_exposure(img, self.brightness_ev);
        }

        let (w, h) = (img.width(), img.height());
        let regions = self.regions.clamp(1, MAX_REGIONS) as f32;
        let radius = ((w.max(h) as f32 / regions) * 0.5).round().max(1.0) as usize;
        let contrast_gain = 1.0 + self.contrast.clamp(-1.0, 1.0);
        let protect_hi = self.protect_highlights.clamp(0.0, 1.0);
        let protect_sh = self.protect_shadows.clamp(0.0, 1.0);
        let brightness = self.brightness_ev.clamp(-MAX_EV, MAX_EV);

        let log_luma: Vec<f32> = img
            .data()
            .chunks_exact(4)
            .map(|p| luma(p).max(LUMA_EPS).log2())
            .collect();
        let target = log_luma.iter().sum::<f32>() / log_luma.len() as f32;
        // Edge-aware local mean: keeps large brightness steps from producing halos.
        let local = crate::guided::guided_filter(&log_luma, &log_luma, w, h, radius, 0.02 * 0.02);

        for (i, px) in img.data_mut().chunks_exact_mut(4).enumerate() {
            let y = luma(px);
            if !y.is_finite() {
                continue;
            }
            let mut lift = (target - local[i]).clamp(-MAX_LIFT_EV, MAX_LIFT_EV) * strength * contrast_gain;
            if lift > 0.0 {
                lift *= 1.0 - protect_hi * smoothstep(0.5, 1.0, y);
            } else if lift < 0.0 {
                lift *= 1.0 - protect_sh * (1.0 - smoothstep(0.0, 0.25, y));
            }
            let gain = (lift + brightness).exp2();
            px[0] *= gain;
            px[1] *= gain;
            px[2] *= gain;
        }
        Ok(())
    }
}

/// Luminance of a linear RGBA pixel. Coefficients are for sRGB primaries, which is this
/// engine's working space.
pub(crate) fn luma(p: &[f32]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}

pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Separable box blur with clamp-to-edge borders. Window is 2r+1 samples.
pub(crate) fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    use rayon::prelude::*;
    let mut rows = vec![0.0f32; w * h];
    rows.par_chunks_mut(w).zip(src.par_chunks(w)).for_each(|(dst, s)| blur_1d(s, dst, r));
    // Columns are independent: blur each in parallel, then scatter back.
    let cols: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let col: Vec<f32> = (0..h).map(|y| rows[y * w + x]).collect();
            let mut out = vec![0.0f32; h];
            blur_1d(&col, &mut out, r);
            out
        })
        .collect();
    let mut out = vec![0.0f32; w * h];
    for (x, col) in cols.iter().enumerate() {
        for (y, v) in col.iter().enumerate() {
            out[y * w + x] = *v;
        }
    }
    out
}

fn blur_1d(src: &[f32], dst: &mut [f32], r: usize) {
    let n = src.len() as isize;
    let at = |i: isize| src[i.clamp(0, n - 1) as usize];
    let ri = r as isize;
    let norm = (2 * r + 1) as f32;
    let mut sum: f32 = (-ri..=ri).map(at).sum();
    for i in 0..n {
        dst[i as usize] = sum / norm;
        sum += at(i + ri + 1) - at(i - ri);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 40x4 image: columns 0..20 use `left`, columns 20..40 use `right` (RGB, opaque).
    fn split_image(left: [u8; 3], right: [u8; 3]) -> LinearImage {
        let mut rgba = Vec::with_capacity(40 * 4 * 4);
        for _y in 0..4 {
            for x in 0..40 {
                let c = if x < 20 { left } else { right };
                rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        LinearImage::from_srgb8(40, 4, &rgba).unwrap()
    }

    fn px(img: &LinearImage, x: usize, y: usize) -> [f32; 4] {
        let i = (y * img.width() + x) * 4;
        let d = &img.data()[i..i + 4];
        [d[0], d[1], d[2], d[3]]
    }

    fn full(strength: f32) -> AdaptiveExposure {
        AdaptiveExposure { strength, regions: 2, ..Default::default() }
    }

    #[test]
    fn neutral_defaults_are_bit_identical() {
        let mut img = split_image([20, 60, 200], [200, 150, 30]);
        let before = img.clone();
        AdaptiveExposure::default().apply(&mut img).unwrap();
        assert_eq!(img, before);
    }

    #[test]
    fn uniform_image_is_unchanged_at_full_strength() {
        let mut img = LinearImage::from_srgb8(8, 8, &[128; 8 * 8 * 4]).unwrap();
        let before = img.to_srgb8();
        full(1.0).apply(&mut img).unwrap();
        for (a, b) in before.iter().zip(img.to_srgb8().iter()) {
            assert!((*a as i32 - *b as i32).abs() <= 1);
        }
    }

    #[test]
    fn dark_side_lifts_and_bright_side_darkens() {
        let mut img = split_image([20, 20, 20], [200, 200, 200]);
        let before = img.clone();
        full(1.0).apply(&mut img).unwrap();
        assert!(px(&img, 2, 1)[0] > px(&before, 2, 1)[0], "dark pixel should brighten");
        assert!(px(&img, 37, 1)[0] < px(&before, 37, 1)[0], "bright pixel should darken");
    }

    #[test]
    fn preserves_hue_by_scaling_rgb_together() {
        let mut img = split_image([40, 80, 20], [200, 160, 120]);
        let before = img.clone();
        full(1.0).apply(&mut img).unwrap();
        for x in [2usize, 37] {
            let a = px(&before, x, 1);
            let b = px(&img, x, 1);
            let g_a = a[0] / a[1];
            let g_b = b[0] / b[1];
            assert!((g_a - g_b).abs() / g_a < 1e-5, "R/G ratio drifted at x={x}");
            assert_eq!(a[3], b[3], "alpha changed at x={x}");
        }
    }

    #[test]
    fn highlight_protection_reduces_positive_lift() {
        // Left 200 (linear Y about 0.58), right 255. Left pixels get a positive lift.
        let mut plain = split_image([200, 200, 200], [255, 255, 255]);
        let mut protected = plain.clone();
        let before = plain.clone();
        full(1.0).apply(&mut plain).unwrap();
        AdaptiveExposure { protect_highlights: 1.0, ..full(1.0) }.apply(&mut protected).unwrap();
        let lift_plain = px(&plain, 2, 1)[0] / px(&before, 2, 1)[0];
        let lift_prot = px(&protected, 2, 1)[0] / px(&before, 2, 1)[0];
        assert!(lift_plain > 1.0, "setup: left pixels should brighten");
        assert!(lift_prot < lift_plain, "protection should reduce the lift");
    }

    #[test]
    fn shadow_protection_reduces_negative_lift_in_shadows() {
        // Left 60 (linear Y about 0.045) sits just above a much darker right side, so pixel
        // x=19 gets a small negative lift.
        let mut plain = split_image([60, 60, 60], [4, 4, 4]);
        let mut protected = plain.clone();
        let before = plain.clone();
        full(1.0).apply(&mut plain).unwrap();
        AdaptiveExposure { protect_shadows: 1.0, ..full(1.0) }.apply(&mut protected).unwrap();
        let out_plain = px(&plain, 19, 1)[0] / px(&before, 19, 1)[0];
        let out_prot = px(&protected, 19, 1)[0] / px(&before, 19, 1)[0];
        assert!(out_plain < 1.0, "setup: pixel should darken without protection");
        assert!(out_prot > out_plain, "shadow protection should reduce darkening");
    }

    #[test]
    fn rejects_non_finite_and_clamps_regions() {
        let mut img = split_image([10, 10, 10], [200, 200, 200]);
        assert_eq!(
            AdaptiveExposure { strength: f32::NAN, ..Default::default() }.apply(&mut img),
            Err(EngineError::NonFiniteParameter("strength"))
        );
        let mut a = split_image([10, 10, 10], [200, 200, 200]);
        let mut b = a.clone();
        AdaptiveExposure { strength: 1.0, regions: 0, ..Default::default() }.apply(&mut a).unwrap();
        AdaptiveExposure { strength: 1.0, regions: 1, ..Default::default() }.apply(&mut b).unwrap();
        assert_eq!(a, b, "regions 0 should clamp to 1");
    }

    #[test]
    fn box_blur_of_constant_is_constant() {
        let src = vec![1.5f32; 5 * 3];
        let out = box_blur(&src, 5, 3, 2);
        assert!(out.iter().all(|v| (v - 1.5).abs() < 1e-6));
    }
}
