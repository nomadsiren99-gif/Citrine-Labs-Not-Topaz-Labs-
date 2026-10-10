//! Noise v1 (spec Milestone 7). Small log-luminance deviations from a blurred base are treated
//! as noise and reduced. Large deviations (edges) are kept. Independent proposal, not a
//! reconstruction of Topaz. Mappings are proposed, not verified.

use crate::adaptive_exposure::luma;
use crate::guided::guided_filter;
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

impl Denoise {
    pub fn apply(&self, img: &mut LinearImage) -> Result<(), EngineError> {
        for (name, v) in [("amount", self.amount), ("suppression", self.suppression)] {
            if !v.is_finite() {
                return Err(EngineError::NonFiniteParameter(name));
            }
        }
        let amount = self.amount.clamp(0.0, 1.0);
        if amount == 0.0 {
            return Ok(());
        }
        let threshold = self.suppression.clamp(0.0, 2.0) * NOISE_FLOOR_EV;
        let (w, h) = (img.width(), img.height());
        let log_luma: Vec<f32> = img.data().chunks_exact(4).map(|p| luma(p).max(LUMA_EPS).log2()).collect();
        // Edge-preserving base: strong edges stay in the base and are not treated as noise.
        let base = guided_filter(&log_luma, &log_luma, w, h, 2, 3e-2);
        for (px, (l, b)) in img.data_mut().chunks_exact_mut(4).zip(log_luma.iter().zip(base)) {
            if !luma(px).is_finite() {
                continue;
            }
            let d = l - b;
            if d.abs() >= threshold {
                continue;
            }
            let g = (-amount * d).exp2();
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
