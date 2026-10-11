//! HDR tone mapping (own method). Luminance is split into a smooth base and a detail layer with an
//! edge-preserving guided filter. Only the base is compressed, in log2 around the image mean, so
//! large-scale light and shadow come down while local texture is kept. This follows the general
//! base/detail idea of Durand and Dorsey (2002); the filter, the compression curve and the highlight
//! shoulder here are this engine's own choices.
//!
//! Controls (all neutral at 0, no change):
//!   strength 0..1: how far the base is compressed (0.6 of the log range at 1).
//!   detail   0..2: gain on the local detail layer (1 keeps it, above 1 adds local contrast).

use crate::adaptive_exposure::luma;
use crate::buffer::LinearImage;
use crate::guided::guided_filter_auto;

/// Largest compression of the base layer at strength 1. Proposed.
const MAX_COMPRESSION: f32 = 0.6;
/// Edge tolerance of the base filter, in log2 luminance. Structure below about 0.2 stop is smoothed
/// into the base. Proposed.
const BASE_EPS: f32 = 0.2 * 0.2;
/// Floor for log2 so black pixels stay finite.
const LUMA_EPS: f32 = 1e-6;
/// Above this channel value, a soft shoulder keeps output below 1 instead of clipping.
const KNEE: f32 = 0.9;

/// Whole-image values a tile must reuse so that it matches the full-frame result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HdrCtx {
    /// Mean log2 luminance of the whole image.
    pub mean: f32,
    /// Full image size, which sets the base filter radius.
    pub full_dims: (usize, usize),
}

/// HDR parameters. `Default` is neutral.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hdr {
    pub strength: f32,
    pub detail: f32,
}

impl Default for Hdr {
    fn default() -> Self {
        Self { strength: 0.0, detail: 1.0 }
    }
}

impl Hdr {
    /// True when the stage changes nothing.
    pub fn is_neutral(&self) -> bool {
        self.strength <= 0.0 && self.detail == 1.0
    }

    /// Base filter radius in pixels for an image of this size.
    pub fn radius(full: (usize, usize)) -> usize {
        (full.0.max(full.1) / 25).max(2)
    }

    /// Applies in place. `ctx` supplies whole-image values for tiles; `None` takes them from `img`.
    pub fn apply_with(&self, img: &mut LinearImage, ctx: Option<HdrCtx>) {
        let strength = self.strength.clamp(0.0, 1.0);
        let detail = self.detail.clamp(0.0, 2.0);
        if strength == 0.0 && detail == 1.0 {
            return;
        }
        let (w, h) = (img.width(), img.height());
        let full = ctx.map(|c| c.full_dims).unwrap_or((w, h));
        let log_luma: Vec<f32> = img.data().chunks_exact(4).map(|p| luma(p).max(LUMA_EPS).log2()).collect();
        let mean = match ctx {
            Some(c) => c.mean,
            None => log_luma.iter().sum::<f32>() / log_luma.len() as f32,
        };
        let base = guided_filter_auto(&log_luma, &log_luma, w, h, Self::radius(full), BASE_EPS);
        let scale = 1.0 - MAX_COMPRESSION * strength;
        for (i, px) in img.data_mut().chunks_exact_mut(4).enumerate() {
            let l = log_luma[i];
            let b = base[i];
            let d = l - b;
            let new_base = mean + (b - mean) * scale;
            let new_l = new_base + d * detail;
            let gain = (new_l - l).exp2();
            for c in 0..3 {
                px[c] = shoulder(px[c] * gain);
            }
        }
    }
}

/// Identity below the knee; above it, a smooth curve that approaches 1 but never reaches it.
fn shoulder(v: f32) -> f32 {
    if !v.is_finite() || v <= 0.0 {
        return 0.0;
    }
    if v <= KNEE {
        return v;
    }
    KNEE + (1.0 - KNEE) * (1.0 - (-(v - KNEE) / (1.0 - KNEE)).exp())
}

/// Sum and count of log2 luminance, so a tiled pass can build the whole-image mean.
pub fn log_luma_sum(img: &LinearImage) -> (f64, usize) {
    let sum: f64 = img.data().chunks_exact(4).map(|p| luma(p).max(LUMA_EPS).log2() as f64).sum();
    (sum, img.data().len() / 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 128x128 light-to-dark ramp in linear light, 0.02 to 0.9.
    fn ramp() -> LinearImage {
        let (w, h) = (128usize, 128usize);
        let mut data = Vec::with_capacity(w * h * 4);
        for _ in 0..h {
            for x in 0..w {
                let v = 0.02 + 0.88 * x as f32 / (w - 1) as f32;
                data.extend_from_slice(&[v, v, v, 1.0]);
            }
        }
        LinearImage::new(w, h, data).unwrap()
    }

    fn range(img: &LinearImage) -> f32 {
        let d = img.data();
        let l = |x: usize| luma(&d[(64 * 128 + x) * 4..]);
        l(120) / l(8)
    }

    #[test]
    fn neutral_is_identity() {
        let base = ramp();
        let mut img = base.clone();
        Hdr::default().apply_with(&mut img, None);
        assert_eq!(img, base);
    }

    #[test]
    fn strength_compresses_the_tonal_range() {
        let base = ramp();
        let mut img = base.clone();
        Hdr { strength: 1.0, detail: 1.0 }.apply_with(&mut img, None);
        let before = range(&base);
        let after = range(&img);
        assert!(after < 0.6 * before, "range {before:.1} -> {after:.1}");
        let rgb_ok = img.data().chunks_exact(4).all(|p| p[..3].iter().all(|v| v.is_finite() && *v >= 0.0 && *v < 1.0));
        assert!(rgb_ok, "colour output must stay below 1");
    }

    #[test]
    fn detail_gain_adds_local_contrast() {
        // Fine texture on a flat grey: detail 2 must enlarge its swing.
        let (w, h) = (64usize, 64usize);
        let mut data = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 0.20 } else { 0.25 };
                data.extend_from_slice(&[v, v, v, 1.0]);
            }
        }
        let base = LinearImage::new(w, h, data).unwrap();
        let swing = |img: &LinearImage| {
            let i = |x: usize| luma(&img.data()[(32 * w + x) * 4..]);
            (i(32) - i(33)).abs()
        };
        let mut img = base.clone();
        Hdr { strength: 0.0, detail: 2.0 }.apply_with(&mut img, None);
        assert!(swing(&img) > 1.5 * swing(&base), "detail gain did not add contrast");
    }

    #[test]
    fn tiled_context_matches_full_frame() {
        let base = ramp();
        let mut full = base.clone();
        let (sum, n) = log_luma_sum(&base);
        let ctx = HdrCtx { mean: (sum / n as f64) as f32, full_dims: (128, 128) };
        let p = Hdr { strength: 0.6, detail: 1.4 };
        p.apply_with(&mut full, None);
        let mut with_ctx = base.clone();
        p.apply_with(&mut with_ctx, Some(ctx));
        let worst = full.data().iter().zip(with_ctx.data()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-5, "context render differs from full frame by {worst}");
    }
}
