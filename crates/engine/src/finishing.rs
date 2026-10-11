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
    /// 0..1. Width of the highlight falloff that feeds the bloom. 0 = narrow, 1 = wide.
    pub diffusion_transition: f32,
    /// 0..1. Darkens toward the edges.
    pub vignette_strength: f32,
    /// 0..1. Higher starts the falloff closer to the edges.
    pub vignette_size: f32,
    /// 0..1. Higher gives a softer falloff.
    pub vignette_transition: f32,
    /// -1..1. Shapes the falloff curve: positive rises faster near the edge, negative more gently.
    pub vignette_curvature: f32,
    /// 0..1. Edge-preserving softening of the whole image, blended in (1 = fully softened half).
    pub diffusion_softness: f32,
    /// Tone zone anchors, ascending in 0..1 (luma). Defaults match the original four zones.
    pub tone_positions: [f32; 4],
    /// Centre of the vignette as fractions of the width and height.
    pub vignette_center: [f32; 2],
    /// 0..1. Monochrome grain amplitude.
    pub grain_amount: f32,
    /// 0..1. 0 = same grain in all channels; 1 = independent grain per channel.
    pub grain_color: f32,
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
            diffusion_transition: 0.5,
            vignette_strength: 0.0,
            vignette_size: 0.5,
            vignette_transition: 0.5,
            vignette_curvature: 0.0,
            diffusion_softness: 0.0,
            tone_positions: [0.1, 0.35, 0.6, 0.9],
            vignette_center: [0.5, 0.5],
            grain_amount: 0.0,
            grain_color: 0.0,
            grain_size: 1.0,
            grain_seed: 1,
            border_size: 0.0,
            border_color: [0.0; 3],
            transparency: 0.0,
        }
    }
}


/// Linear sRGB to XYZ (D65), and its inverse, from the sRGB specification.
const SRGB_TO_XYZ: [f32; 9] = [0.412_456_4, 0.357_576_1, 0.180_437_5, 0.212_672_9, 0.715_152_2, 0.072_175, 0.019_333_9, 0.119_192, 0.950_304_1];
/// Bradford cone response and its inverse.
const BRADFORD: [f32; 9] = [0.8951, 0.2664, -0.1614, -0.7502, 1.7135, 0.0367, 0.0389, -0.0685, 1.0296];
const BRADFORD_INV: [f32; 9] = [0.986_993, -0.147_054, 0.159_963, 0.432_305, 0.518_36, 0.049_291, -0.008_529, 0.040_043, 0.968_487];

fn mul3(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
    let mut out = [0.0f32; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    out
}

fn mul_vec(m: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

fn inv3(m: &[f32; 9]) -> [f32; 9] {
    let (a, b, c, d, e, f, g, h, i) = (m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8]);
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        (e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det,
        (f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det,
        (d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det,
    ]
}

/// White point (XYZ, Y = 1) of a correlated colour temperature, Kim et al. approximation on
/// the Planckian locus, valid from 4000 K to 25000 K (the 4000 K and up coefficient set).
fn white_xyz(kelvin: f32) -> [f32; 3] {
    let t = kelvin.clamp(4000.0, 25000.0);
    let x = -3.025_846_9e9 / (t * t * t) + 2.107_037_9e6 / (t * t) + 0.222_634_7e3 / t + 0.240_390;
    let y = 3.081_758_0 * x * x * x - 5.873_386_7 * x * x + 3.751_129_97 * x - 0.370_014_83;
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// Linear RGB matrix that moves the white point from D65 to the colour temperature set by
/// `warm` (+1 = 4000 K, warmer; -1 = 9000 K, cooler), by a Bradford chromatic adaptation.
/// Returns `None` at zero, so the neutral setting is an exact identity.
fn warmth_matrix(warm: f32) -> Option<[f32; 9]> {
    if warm == 0.0 {
        return None;
    }
    let d65 = white_xyz(6504.0);
    let target = white_xyz(6500.0 - 2500.0 * warm);
    let src = mul_vec(&BRADFORD, d65);
    let dst = mul_vec(&BRADFORD, target);
    let scale = [dst[0] / src[0], 0.0, 0.0, 0.0, dst[1] / src[1], 0.0, 0.0, 0.0, dst[2] / src[2]];
    let adapt_xyz = mul3(&BRADFORD_INV, &mul3(&scale, &BRADFORD));
    let xyz_to_srgb = inv3(&SRGB_TO_XYZ);
    Some(mul3(&xyz_to_srgb, &mul3(&adapt_xyz, &SRGB_TO_XYZ)))
}

impl Finishing {
    /// Applies in place. `original` is the image before the global adjustments, used by
    /// transparency. Out-of-range values are clamped; non-finite values are rejected first.
    pub fn apply(&self, img: &mut LinearImage, original: &LinearImage) -> Result<(), EngineError> {
        let region = Region { origin: (0, 0), full: (img.width(), img.height()) };
        self.apply_region(img, original, region)
    }

    /// Like `apply`, for a tile: `img` and `original` are the tile (with halo), `region` gives where
    /// it sits in the full image, so position-dependent effects match the full-frame result.
    pub fn apply_region(&self, img: &mut LinearImage, original: &LinearImage, region: Region) -> Result<(), EngineError> {
        let scalars = [self.warmth, self.tone_strength, self.diffusion, self.diffusion_transition,
            self.vignette_strength, self.vignette_size, self.vignette_transition, self.grain_amount,
            self.grain_color, self.grain_size, self.border_size, self.transparency];
        if let Some(i) = scalars.iter().position(|v| !v.is_finite()) {
            return Err(EngineError::NonFiniteParameter(["warmth", "tone_strength", "diffusion",
                "diffusion_transition", "vignette_strength", "vignette_size", "vignette_transition",
                "grain_amount", "grain_color", "grain_size", "border_size", "transparency"][i]));
        }
        if img.width() != original.width() || img.height() != original.height() {
            return Err(EngineError::InvalidDimensions { width: original.width(), height: original.height() });
        }
        let (w, h) = (img.width(), img.height());
        let (x0, y0) = region.origin;
        let (fw, fh) = region.full;
        let untouched = self.warmth == 0.0 && self.tone_strength == 0.0 && self.diffusion == 0.0
            && self.diffusion_softness == 0.0
            && self.vignette_strength == 0.0 && self.grain_amount == 0.0
            && self.border_size == 0.0 && self.transparency == 0.0;
        if untouched {
            return Ok(());
        }

        let warm = self.warmth.clamp(-1.0, 1.0);
        let warm_matrix = warmth_matrix(warm);
        let tone = self.tone_strength.clamp(0.0, 1.0);
        let diff = self.diffusion.clamp(0.0, 1.0);
        let vig = self.vignette_strength.clamp(0.0, 1.0);
        let vsize = self.vignette_size.clamp(0.0, 1.0);
        let vtrans = self.vignette_transition.clamp(0.0, 1.0);
        let grain = self.grain_amount.clamp(0.0, 1.0);
        let color = self.grain_color.clamp(0.0, 1.0);
        let cell = self.grain_size.clamp(1.0, 8.0).round() as u32;
        let border = self.border_size.clamp(0.0, 0.2) * fw.min(fh) as f32;
        let t = self.transparency.clamp(0.0, 1.0);

        // Softness: edge-preserving blend toward a smoothed copy, so facial contours hold.
        let soft = self.diffusion_softness.clamp(0.0, 1.0) * 0.5;
        if soft > 0.0 {
            soften(img, soft, (fw.max(fh) / 100).max(1));
        }

        // Diffusion: blur the highlights and add them back.
        let bloom = if diff > 0.0 {
            Some(bloom_layer(img, (fw.max(fh) / 50).max(1), self.diffusion_transition))
        } else {
            None
        };

        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let px = &mut img.data_mut()[i..i + 4];
                let (xg, yg) = (x + x0, y + y0);
                let fx = xg as f32 / fw as f32;
                let fy = yg as f32 / fh as f32;

                if let Some(m) = warm_matrix {
                    let [r, g, b] = [px[0], px[1], px[2]];
                    px[0] = m[0] * r + m[1] * g + m[2] * b;
                    px[1] = m[3] * r + m[4] * g + m[5] * b;
                    px[2] = m[6] * r + m[7] * g + m[8] * b;
                }

                if tone > 0.0 {
                    let l = luma(px).clamp(0.0, 1.0);
                    let mut tint = [0.0f32; 3];
                    let mut wsum = 0.0;
                    for (k, anchor) in self.tone_positions.iter().enumerate() {
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
                    // Curvature reshapes the radial response: exponent 2^-c, so c = 0 is unchanged.
                    let f = smoothstep(start, start + width, r).powf(2f32.powf(-self.vignette_curvature.clamp(-1.0, 1.0)));
                    // Darkening stops at 80%: full strength must not crush the corners to black.
                    let g = 1.0 - 0.8 * vig * f;
                    px[0] *= g;
                    px[1] *= g;
                    px[2] *= g;
                }

                if grain > 0.0 {
                    // Shared grain value, mixed with a per-channel value by grain_color.
                    let (gx, gy) = ((xg as u32) / cell, (yg as u32) / cell);
                    let shared = hash_signed(gx, gy, self.grain_seed);
                    for c in 0..3 {
                        let own = hash_signed(gx, gy, self.grain_seed.wrapping_add(1 + c as u32 * 7919));
                        let n = shared + color * (own - shared);
                        px[c] *= 1.0 + grain * 0.2 * n;
                    }
                }

                if border > 0.0 {
                    let edge = (xg as f32).min((fw - 1 - xg) as f32).min(yg as f32).min((fh - 1 - yg) as f32);
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
/// Where a tile sits in the full image: its top-left corner and the full size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub origin: (usize, usize),
    pub full: (usize, usize),
}

/// Blends each channel toward a guided-filtered copy, guided by luma, by `amount` (0..0.5).
fn soften(img: &mut LinearImage, amount: f32, radius: usize) {
    let (w, h) = (img.width(), img.height());
    let luma_vec: Vec<f32> = img.data().chunks_exact(4).map(luma).collect();
    for c in 0..3 {
        let chan: Vec<f32> = img.data().chunks_exact(4).map(|p| p[c]).collect();
        let smooth = crate::guided::guided_filter_auto(&luma_vec, &chan, w, h, radius, 0.01);
        for (i, px) in img.data_mut().chunks_exact_mut(4).enumerate() {
            px[c] = px[c] * (1.0 - amount) + smooth[i] * amount;
        }
    }
}

fn bloom_layer(img: &LinearImage, radius: usize, transition: f32) -> Vec<f32> {
    let (w, h) = (img.width(), img.height());
    let n = w * h;
    let mut out = vec![0.0f32; n * 3];
    // Falloff starts at 0.7 luma and widens with transition: no hard cutoff in the glow.
    // Minimum width 0.15 keeps even the narrowest setting smooth; wider falloff reaches 0.5.
    let width = 0.15 + 0.35 * transition.clamp(0.0, 1.0);
    let mask: Vec<f32> = img.data().chunks_exact(4).map(|p| smoothstep(0.7, 0.7 + width, luma(p))).collect();
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

#[cfg(test)]
mod gate5_tests {
    //! Gate 5: warmth is a white-point adaptation in linear light, not a red-blue shift.
    use super::*;

    /// Linear RGB of a white reference after the warmth matrix (input is D65 white).
    fn white_after(warm: f32) -> [f32; 3] {
        let m = warmth_matrix(warm).unwrap();
        mul_vec(&m, [1.0, 1.0, 1.0])
    }

    #[test]
    fn warm_setting_makes_white_warmer_and_cool_makes_it_cooler() {
        let warm = white_after(0.8);
        let cool = white_after(-0.8);
        assert!(warm[0] / warm[2] > 1.05, "warm white R/B {:.3}", warm[0] / warm[2]);
        assert!(cool[0] / cool[2] < 0.95, "cool white R/B {:.3}", cool[0] / cool[2]);
    }

    #[test]
    fn white_point_matches_the_kelvin_target() {
        // Reference: CIE xy of 4000 K (0.3805, 0.3769) converted to linear sRGB gives R/G 1.53
        // and B/G 0.58. The Kim approximation and Bradford step land within a few percent.
        let w = white_after(1.0);
        let (rg, bg) = (w[0] / w[1], w[2] / w[1]);
        assert!((1.45..1.60).contains(&rg), "R/G at 4000 K {rg:.3}");
        assert!((0.52..0.64).contains(&bg), "B/G at 4000 K {bg:.3}");
    }

    #[test]
    fn adaptation_keeps_luminance_of_white() {
        for warm in [-1.0f32, -0.4, 0.4, 1.0] {
            let w = white_after(warm);
            let y = 0.2126 * w[0] + 0.7152 * w[1] + 0.0722 * w[2];
            assert!((y - 1.0).abs() < 0.02, "warm {warm}: luminance {y:.4}");
        }
    }
}

#[cfg(test)]
mod gate5_vignette_tests {
    //! Gate 5: a full-strength vignette keeps the corners above black and falls off smoothly.
    use super::*;

    #[test]
    fn full_vignette_keeps_corners_off_black_and_is_smooth() {
        let (w, h) = (64usize, 64usize);
        let grey = LinearImage::new(w, h, vec![0.5; w * h * 4]).unwrap();
        let mut img = grey.clone();
        let f = Finishing { vignette_strength: 1.0, ..Default::default() };
        f.apply(&mut img, &grey).unwrap();
        let lum = |x: usize, y: usize| luma(&img.data()[(y * w + x) * 4..]);
        let corner = lum(0, 0);
        assert!(corner >= 0.5 * 0.2 - 1e-3, "corner collapsed to {corner:.3}");
        let max_step = (0..h - 1)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| (lum(x, y + 1) - lum(x, y)).abs())
            .fold(0.0f32, f32::max);
        assert!(max_step < 0.05, "vignette step {max_step:.3} per pixel");
    }
}

#[cfg(test)]
mod gate5_grain_tests {
    //! Gate 5: grain colour sets chromatic correlation; diffusion transition widens the falloff.
    use super::*;

    /// Correlation of the R and G deviations from 1 across a grid of grain samples.
    fn rg_correlation(img: &LinearImage) -> f32 {
        let d: Vec<(f32, f32)> = img.data().chunks_exact(4).map(|p| (p[0], p[1])).collect();
        let n = d.len() as f32;
        let (mr, mg) = (d.iter().map(|v| v.0).sum::<f32>() / n, d.iter().map(|v| v.1).sum::<f32>() / n);
        let cov: f32 = d.iter().map(|v| (v.0 - mr) * (v.1 - mg)).sum();
        let vr: f32 = d.iter().map(|v| (v.0 - mr).powi(2)).sum();
        let vg: f32 = d.iter().map(|v| (v.1 - mg).powi(2)).sum();
        cov / (vr * vg).sqrt()
    }

    #[test]
    fn grain_colour_controls_channel_correlation() {
        let (w, h) = (128usize, 128usize);
        let grey = LinearImage::new(w, h, vec![0.5; w * h * 4]).unwrap();
        let mono = {
            let mut img = grey.clone();
            Finishing { grain_amount: 1.0, grain_color: 0.0, grain_seed: 3, ..Default::default() }
                .apply(&mut img, &grey)
                .unwrap();
            rg_correlation(&img)
        };
        let colour = {
            let mut img = grey.clone();
            Finishing { grain_amount: 1.0, grain_color: 1.0, grain_seed: 3, ..Default::default() }
                .apply(&mut img, &grey)
                .unwrap();
            rg_correlation(&img)
        };
        assert!(mono > 0.99, "monochrome grain correlation {mono:.3}");
        assert!(colour.abs() < 0.3, "chromatic grain correlation {colour:.3}");
    }

    #[test]
    fn diffusion_transition_has_no_hard_cutoff() {
        // Luma ramp from 0.5 to 1.0 across the width; the bloom must rise without a jump.
        let (w, h) = (200usize, 4usize);
        let mut data = Vec::with_capacity(w * h * 4);
        for _ in 0..h {
            for x in 0..w {
                let v = 0.5 + 0.5 * x as f32 / (w - 1) as f32;
                data.extend_from_slice(&[v, v, v, 1.0]);
            }
        }
        let original = LinearImage::new(w, h, data).unwrap();
        let mut steps = Vec::new();
        for transition in [0.0f32, 0.5, 1.0] {
            let mut img = original.clone();
            Finishing { diffusion: 1.0, diffusion_transition: transition, ..Default::default() }
                .apply(&mut img, &original)
                .unwrap();
            let added: Vec<f32> = (0..w)
                .map(|x| luma(&img.data()[(2 * w + x) * 4..]) - luma(&original.data()[(2 * w + x) * 4..]))
                .collect();
            let max_step = added.windows(2).map(|p| (p[1] - p[0]).abs()).fold(0.0f32, f32::max);
            // The narrowest falloff still rises over 0.15 luma, so the bloom changes by about 0.02
            // per pixel at most; anything near a step would be far above this.
            assert!(max_step < 0.03, "transition {transition}: bloom jumps {max_step:.4} per pixel");
            steps.push(max_step);
        }
        assert!(steps[2] < steps[0], "wider falloff should be gentler: {steps:?}");
    }
}

#[cfg(test)]
mod gate5_tone_tests {
    //! Gate 5: a tone colour tints only its own luminance zone.
    use super::*;

    #[test]
    fn red_tint_on_the_midtone_zone_leaves_highlights_alone() {
        // Two halves: mid-grey (luma about 0.35, the second tone anchor) and near-white (0.9).
        let (w, h) = (2usize, 1usize);
        let mid = 0.35f32;
        let high = 0.9f32;
        let original = LinearImage::new(w, h, vec![mid, mid, mid, 1.0, high, high, high, 1.0]).unwrap();
        let mut img = original.clone();
        let mut tone_colors = [[1.0f32; 3]; 4];
        tone_colors[1] = [1.0, 0.5, 0.5]; // red tint on the midtone anchor only
        Finishing { tone_colors, tone_strength: 1.0, ..Default::default() }
            .apply(&mut img, &original)
            .unwrap();
        let d = img.data();
        assert!(d[0] > d[1] * 1.3, "midtone not tinted: R {} G {}", d[0], d[1]);
        assert!((d[4] - d[5]).abs() < 1e-3 && (d[4] - high).abs() < 0.02, "highlight changed: {:?}", &d[4..8]);
    }
}

/// Border "Pick from Image": average linear RGB over a square window centred on a point.
/// Coordinates are normalised to 0..1, origin at the top-left, x to the right, y down. The point
/// is clamped to the image. `half_window` is in pixels (0 = single pixel).
pub fn sample_colour(img: &LinearImage, fx: f32, fy: f32, half_window: usize) -> [f32; 3] {
    let (w, h) = (img.width(), img.height());
    let cx = (fx.clamp(0.0, 1.0) * (w - 1) as f32).round() as usize;
    let cy = (fy.clamp(0.0, 1.0) * (h - 1) as f32).round() as usize;
    let (x0, x1) = (cx.saturating_sub(half_window), (cx + half_window).min(w - 1));
    let (y0, y1) = (cy.saturating_sub(half_window), (cy + half_window).min(h - 1));
    let mut sum = [0.0f32; 3];
    let mut n = 0.0f32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let i = (y * w + x) * 4;
            for c in 0..3 {
                sum[c] += img.data()[i + c];
            }
            n += 1.0;
        }
    }
    [sum[0] / n, sum[1] / n, sum[2] / n]
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    #[test]
    fn picks_the_colour_at_the_normalised_point() {
        // 4x4: top-left quadrant red, the rest blue.
        let mut data = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                let c = if x < 2 && y < 2 { [0.8, 0.1, 0.1] } else { [0.1, 0.1, 0.8] };
                data.extend_from_slice(&[c[0], c[1], c[2], 1.0]);
            }
        }
        let img = LinearImage::new(4, 4, data).unwrap();
        let red = sample_colour(&img, 0.0, 0.0, 0);
        assert!((red[0] - 0.8).abs() < 1e-6, "top-left should be red, got {red:?}");
        let blue = sample_colour(&img, 1.0, 1.0, 0);
        assert!((blue[2] - 0.8).abs() < 1e-6, "bottom-right should be blue, got {blue:?}");
        let clamped = sample_colour(&img, 5.0, -3.0, 0);
        assert_eq!(clamped, sample_colour(&img, 1.0, 0.0, 0), "points outside the image clamp to its edge");
    }
}
