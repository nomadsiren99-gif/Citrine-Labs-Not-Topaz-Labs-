//! Noise v1 (spec Milestone 7). Small log-luminance deviations from a blurred base are treated
//! as noise and reduced. Large deviations (edges) are kept. Independent proposal, not a
//! reconstruction of Topaz. Mappings are proposed, not verified.

use crate::adaptive_exposure::luma;
use crate::buffer::{EngineError, LinearImage};

/// Deviations below `suppression * NOISE_FLOOR_EV` are treated as noise. Proposed.
const NOISE_FLOOR_EV: f32 = 0.05;
const LUMA_EPS: f32 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Denoise {
    /// 0..1. Blend of the noise removal. 0 = off.
    pub amount: f32,
    /// 0..2. Scales the threshold below which deviations count as noise. 1 = baseline.
    pub suppression: f32,
}

impl Default for Denoise {
    fn default() -> Self {
        Self { amount: 0.0, suppression: 1.0 }
    }
}

/// Log2-luminance minus its edge-preserving base, one value per pixel. Shared with the tiled path.
pub(crate) fn residue(img: &LinearImage) -> Vec<f32> {
    let (w, h) = (img.width(), img.height());
    let log_luma: Vec<f32> = img.data().chunks_exact(4).map(|p| luma(p).max(LUMA_EPS).log2()).collect();
    // Edge-preserving base: strong edges stay in the base and are not treated as noise.
    let base = crate::guided::guided_filter_auto(&log_luma, &log_luma, w, h, 2, 3e-2);
    log_luma.iter().zip(&base).map(|(l, b)| l - b).collect()
}

impl Denoise {
    pub fn apply(&self, img: &mut LinearImage) -> Result<(), EngineError> {
        self.apply_with(img, None)
    }

    /// Like `apply`, with an optional whole-image noise level (see `tiled`).
    pub fn apply_with(&self, img: &mut LinearImage, sigma: Option<f32>) -> Result<(), EngineError> {
        for (name, v) in [("amount", self.amount), ("suppression", self.suppression)] {
            if !v.is_finite() {
                return Err(EngineError::NonFiniteParameter(name));
            }
        }
        let amount = self.amount.clamp(0.0, 1.0);
        if amount == 0.0 {
            return Ok(());
        }
        let residue = residue(img);
        // Smooth weight, not a hard cut: deviations near the image's own noise level are fully
        // corrected, and weight falls to zero by 4 sigma so edges and texture are left alone.
        // A hard cut at a fixed level left flat-patch variance at 0.98x in the gate 4 test.
        let sigma = sigma.unwrap_or_else(|| crate::details::noise_sigma(&residue)).max(NOISE_FLOOR_EV * 0.01);
        let k = self.suppression.clamp(0.0, 2.0);
        let (t0, t1) = (2.0 * sigma * k, 4.0 * sigma * k);
        for (px, &d) in img.data_mut().chunks_exact_mut(4).zip(&residue) {
            if !luma(px).is_finite() {
                continue;
            }
            let weight = 1.0 - crate::adaptive_exposure::smoothstep(t0, t1, d.abs());
            if weight <= 0.0 {
                continue;
            }
            let g = (-amount * weight * d).exp2();
            px[0] *= g;
            px[1] *= g;
            px[2] *= g;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noisy_flat() -> LinearImage {
        let mut rgba = Vec::new();
        for i in 0..(16 * 16) {
            let v = if i % 2 == 0 { 118 } else { 122 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
        LinearImage::from_srgb8(16, 16, &rgba).unwrap()
    }

    fn spread(img: &LinearImage) -> f32 {
        let l: Vec<f32> = img.data().chunks_exact(4).map(|p| luma(p)).collect();
        let max = l.iter().cloned().fold(f32::MIN, f32::max);
        let min = l.iter().cloned().fold(f32::MAX, f32::min);
        max - min
    }

    #[test]
    fn zero_amount_is_bit_identical() {
        let mut img = noisy_flat();
        let before = img.clone();
        Denoise::default().apply(&mut img).unwrap();
        assert_eq!(img, before);
    }

    #[test]
    fn removes_small_fluctuations() {
        let mut img = noisy_flat();
        let before = spread(&img);
        Denoise { amount: 1.0, suppression: 2.0 }.apply(&mut img).unwrap();
        assert!(spread(&img) < before * 0.5);
    }

    #[test]
    fn keeps_a_strong_edge() {
        let mut rgba = Vec::new();
        for _y in 0..4 {
            for x in 0..16 {
                let v = if x < 8 { 40 } else { 220 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let original = LinearImage::from_srgb8(16, 4, &rgba).unwrap();
        let mut img = original.clone();
        Denoise { amount: 1.0, suppression: 1.0 }.apply(&mut img).unwrap();
        let i = (2 * 16 + 12) * 4;
        assert!((luma(&img.data()[i..i + 4]) - luma(&original.data()[i..i + 4])).abs() < 1e-4);
    }
}

#[cfg(test)]
mod gate4_tests {
    //! Gate 4: denoise cuts flat-patch variance but keeps a strong edge visible.
    use super::*;

    fn std_of(v: &[f32]) -> f32 {
        let m = v.iter().sum::<f32>() / v.len() as f32;
        (v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len() as f32).sqrt()
    }

    #[test]
    fn flat_patch_variance_drops_and_step_edge_survives() {
        let mut s: u32 = 31337;
        let mut rgba = Vec::new();
        for _y in 0..64 {
            for x in 0..64 {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let n = (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                let base = if x < 32 { 60.0 } else { 180.0 };
                let v = (base + 8.0 * n).round().clamp(0.0, 255.0) as u8;
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let original = LinearImage::from_srgb8(64, 64, &rgba).unwrap();
        let mut img = original.clone();
        Denoise { amount: 1.0, suppression: 1.0 }.apply(&mut img).unwrap();

        let flat = |i: &LinearImage| {
            let v: Vec<f32> = (0..64)
                .flat_map(|y| (4..28).map(move |x| (y, x)))
                .map(|(y, x)| luma(&i.data()[(y * 64 + x) * 4..]))
                .collect();
            std_of(&v)
        };
        let ratio = flat(&img) / flat(&original);
        assert!(ratio < 0.7, "flat-patch std only fell to {ratio:.2}x");

        let edge = |i: &LinearImage| luma(&i.data()[(10 * 64 + 36) * 4..]) - luma(&i.data()[(10 * 64 + 27) * 4..]);
        let kept = edge(&img) / edge(&original);
        assert!(kept > 0.9, "step edge lost {:.0}% of its contrast", (1.0 - kept) * 100.0);
    }
}
