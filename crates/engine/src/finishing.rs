//! Finishing v1 (spec Milestone 9): warmth, four-zone tone, diffusion, vignette, grain,
//! border, transparency. Independent proposals, not reconstructions of Topaz. Mappings are
//! proposed, not verified. Each control is neutral at its default.

use crate::adaptive_exposure::{box_blur, luma, smoothstep};
use crate::buffer::{EngineError, LinearImage};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finishing {
    /// -1..1. Negative cools, positive warms. Proposed scale: +/-10% on R and B.
    pub warmth: f32,
    /// Four tone colours (linear RGB) and their overall strength 0..1. Neutral white = no change.
    pub tone_colors: [[f32; 3]; 4],
    pub tone_strength: f32,
    /// 0..1. Bloom of highlights, blurred and added back.
    pub diffusion: f32,
    /// 0..1. Darkens toward the edges.
    pub vignette_strength: f32,
    /// 0..1. Higher starts the falloff closer to the edges.
    pub vignette_size: f32,
    /// 0..1. Higher gives a softer falloff.
    pub vignette_transition: f32,
    /// Centre of the vignette as fractions of the width and height.
    pub vignette_center: [f32; 2],
    /// 0..1. Monochrome grain amplitude.
    pub grain_amount: f32,
    /// Grain cell size in pixels, 1..8.
    pub grain_size: f32,
    pub grain_seed: u32,
    /// 0..0.2 of the shorter side. Frame drawn inside the image.
    pub border_size: f32,
    pub border_color: [f32; 3],
    /// 0..1. Blends the whole result back toward `original` (1 = original).
    pub transparency: f32,
}

impl Default for Finishing {
    fn default() -> Self {
        Self {
            warmth: 0.0,
            tone_colors: [[1.0; 3]; 4],
            tone_strength: 0.0,
            diffusion: 0.0,
            vignette_strength: 0.0,
            vignette_size: 0.5,
            vignette_transition: 0.5,
            vignette_center: [0.5, 0.5],
            grain_amount: 0.0,
            grain_size: 1.0,
            grain_seed: 1,
            border_size: 0.0,
            border_color: [0.0; 3],
            transparency: 0.0,
        }
    }
}

const TONE_ANCHORS: [f32; 4] = [0.1, 0.35, 0.6, 0.9];

impl Finishing {
    /// Applies in place. `original` is the image before the global adjustments, used by
    /// transparency. Out-of-range values are clamped; non-finite values are rejected first.
    pub fn apply(&self, img: &mut LinearImage, original: &LinearImage) -> Result<(), EngineError> {
        let scalars = [self.warmth, self.tone_strength, self.diffusion, self.vignette_strength,
            self.vignette_size, self.vignette_transition, self.grain_amount, self.grain_size,
            self.border_size, self.transparency];
        if let Some(i) = scalars.iter().position(|v| !v.is_finite()) {
            return Err(EngineError::NonFiniteParameter(["warmth", "tone_strength", "diffusion",
                "vignette_strength", "vignette_size", "vignette_transition", "grain_amount",
                "grain_size", "border_size", "transparency"][i]));
        }
        if img.width() != original.width() || img.height() != original.height() {
            return Err(EngineError::InvalidDimensions { width: original.width(), height: original.height() });
        }
        let (w, h) = (img.width(), img.height());
        let untouched = self.warmth == 0.0 && self.tone_strength == 0.0 && self.diffusion == 0.0
            && self.vignette_strength == 0.0 && self.grain_amount == 0.0
            && self.border_size == 0.0 && self.transparency == 0.0;
        if untouched {
            return Ok(());
        }

        let warm = self.warmth.clamp(-1.0, 1.0) * 0.1;
        let tone = self.tone_strength.clamp(0.0, 1.0);
        let diff = self.diffusion.clamp(0.0, 1.0);
        let vig = self.vignette_strength.clamp(0.0, 1.0);
        let vsize = self.vignette_size.clamp(0.0, 1.0);
        let vtrans = self.vignette_transition.clamp(0.0, 1.0);
        let grain = self.grain_amount.clamp(0.0, 1.0);
        let cell = self.grain_size.clamp(1.0, 8.0).round() as u32;
        let border = self.border_size.clamp(0.0, 0.2) * w.min(h) as f32;
        let t = self.transparency.clamp(0.0, 1.0);

        // Diffusion: blur the highlights and add them back.
        let bloom = if diff > 0.0 { Some(bloom_layer(img, (w.max(h) / 50).max(1))) } else { None };

        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let px = &mut img.data_mut()[i..i + 4];
                let fx = x as f32 / w as f32;
                let fy = y as f32 / h as f32;

                px[0] *= 1.0 + warm;
                px[2] *= 1.0 - warm;

                if tone > 0.0 {
                    let l = luma(px).clamp(0.0, 1.0);
                    let mut tint = [0.0f32; 3];
                    let mut wsum = 0.0;
                    for (k, anchor) in TONE_ANCHORS.iter().enumerate() {
                        let wk = (1.0 - ((l - anchor).abs() / 0.25)).max(0.0);
                        wsum += wk;
                        for c in 0..3 {
                            tint[c] += wk * self.tone_colors[k][c];
                        }
                    }
                    if wsum > 0.0 {
                        for c in 0..3 {
                            px[c] *= 1.0 + tone * (tint[c] / wsum - 1.0);
                        }
                    }
                }

                if let Some(ref b) = bloom {
                    for c in 0..3 {
                        px[c] += diff * b[(y * w + x) * 3 + c];
                    }
                }

                if vig > 0.0 {
                    let dx = fx - self.vignette_center[0];
                    let dy = fy - self.vignette_center[1];
                    let r = (dx * dx + dy * dy).sqrt() / std::f32::consts::FRAC_1_SQRT_2;
                    let start = 0.9 - 0.7 * vsize;
                    let width = 0.05 + 0.5 * vtrans;
                    let f = smoothstep(start, start + width, r);
                    let g = 1.0 - vig * f;
                    px[0] *= g;
                    px[1] *= g;
                    px[2] *= g;
                }

                if grain > 0.0 {
                    let n = hash_signed((x as u32) / cell, (y as u32) / cell, self.grain_seed);
                    let g = 1.0 + grain * 0.2 * n;
                    px[0] *= g;
                    px[1] *= g;
                    px[2] *= g;
                }

                if border > 0.0 {
                    let edge = (x as f32).min((w - 1 - x) as f32).min(y as f32).min((h - 1 - y) as f32);
                    if edge < border {
                        px[0] = self.border_color[0];
                        px[1] = self.border_color[1];
                        px[2] = self.border_color[2];
                    }
                }

                if t > 0.0 {
                    let o = &original.data()[i..i + 3];
                    for c in 0..3 {
                        px[c] = px[c] * (1.0 - t) + o[c] * t;
                    }
                }
                px[0] = px[0].max(0.0);
                px[1] = px[1].max(0.0);
                px[2] = px[2].max(0.0);
            }
        }
        Ok(())
    }
}

/// Blurred copy of the highlights, RGB, one value per pixel per channel.
fn bloom_layer(img: &LinearImage, radius: usize) -> Vec<f32> {
    let (w, h) = (img.width(), img.height());
    let n = w * h;
    let mut out = vec![0.0f32; n * 3];
    let mask: Vec<f32> = img.data().chunks_exact(4).map(|p| smoothstep(0.7, 1.0, luma(p))).collect();
    for c in 0..3 {
        let chan: Vec<f32> = img.data().chunks_exact(4).enumerate().map(|(i, p)| p[c] * mask[i]).collect();
        let blurred = box_blur(&chan, w, h, radius);
        for i in 0..n {
            out[i * 3 + c] = blurred[i];
        }
    }
    out
}

/// Deterministic hash on integer cell coordinates, mapped to -1..1. Depends only on
/// coordinates and seed, so tiled and full-frame renders agree.
fn hash_signed(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(v: u8) -> LinearImage {
        LinearImage::from_srgb8(16, 12, &vec![v; 16 * 12 * 4]).unwrap()
    }

    fn lum_at(img: &LinearImage, x: usize, y: usize) -> f32 {
        let i = (y * img.width() + x) * 4;
        luma(&img.data()[i..i + 4])
    }

    #[test]
    fn neutral_defaults_are_bit_identical() {
        let original = flat(120);
        let mut img = flat(120);
        Finishing::default().apply(&mut img, &original).unwrap();
        assert_eq!(img, original);
    }

    #[test]
    fn warmth_raises_red_and_lowers_blue() {
        let original = flat(120);
        let mut img = flat(120);
        Finishing { warmth: 1.0, ..Default::default() }.apply(&mut img, &original).unwrap();
        let i = 0;
        assert!(img.data()[i] > original.data()[i]);
        assert!(img.data()[i + 2] < original.data()[i + 2]);
    }

    #[test]
    fn full_transparency_returns_the_original() {
        let original = flat(90);
        let mut img = flat(90);
        Finishing { warmth: 1.0, transparency: 1.0, ..Default::default() }.apply(&mut img, &original).unwrap();
        assert_eq!(img, original);
    }

    #[test]
    fn vignette_darkens_corners_not_centre() {
        let original = flat(150);
        let mut img = flat(150);
        Finishing { vignette_strength: 1.0, ..Default::default() }.apply(&mut img, &original).unwrap();
        assert!(lum_at(&img, 0, 0) < lum_at(&img, 8, 6) * 0.99);
        assert!((lum_at(&img, 8, 6) - lum_at(&original, 8, 6)).abs() < 1e-5);
    }

    #[test]
    fn grain_is_deterministic_and_zero_amount_is_identity() {
        let original = flat(100);
        let mut a = flat(100);
        let mut b = flat(100);
        let f = Finishing { grain_amount: 0.5, grain_seed: 7, ..Default::default() };
        f.apply(&mut a, &original).unwrap();
        f.apply(&mut b, &original).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, original);
    }

    #[test]
    fn border_colours_the_edge_only() {
        let original = flat(120);
        let mut img = flat(120);
        Finishing { border_size: 0.1, border_color: [1.0, 0.0, 0.0], ..Default::default() }
            .apply(&mut img, &original)
            .unwrap();
        assert_eq!(&img.data()[0..3], &[1.0, 0.0, 0.0]);
        let centre = (6 * 16 + 8) * 4;
        assert_eq!(&img.data()[centre..centre + 3], &original.data()[centre..centre + 3]);
    }

    #[test]
    fn diffusion_spreads_light_from_a_highlight() {
        let mut rgba = vec![40u8; 16 * 12 * 4];
        let spot = (6 * 16 + 8) * 4;
        rgba[spot..spot + 4].copy_from_slice(&[255, 255, 255, 255]);
        let original = LinearImage::from_srgb8(16, 12, &rgba).unwrap();
        let mut img = original.clone();
        Finishing { diffusion: 1.0, ..Default::default() }.apply(&mut img, &original).unwrap();
        assert!(lum_at(&img, 9, 6) > lum_at(&original, 9, 6));
    }

    #[test]
    fn tone_with_white_colours_is_neutral() {
        let original = flat(100);
        let mut img = flat(100);
        Finishing { tone_strength: 1.0, ..Default::default() }.apply(&mut img, &original).unwrap();
        assert_eq!(img, original);
    }

    #[test]
    fn rejects_non_finite_parameter() {
        let original = flat(100);
        let mut img = flat(100);
        assert_eq!(
            Finishing { warmth: f32::NAN, ..Default::default() }.apply(&mut img, &original),
            Err(EngineError::NonFiniteParameter("warmth"))
        );
    }
}
