//! Local brush tools v1 (spec Milestone 8). Each stroke is a soft circular mask with a kind:
//! Dodge, Burn, Smooth or Brush Out. Strokes are applied in order. Independent proposal, not a
//! reconstruction of Topaz. A brush UI is not built yet; masks come from parameters.

use crate::adaptive_exposure::{box_blur, smoothstep};
use crate::buffer::{EngineError, LinearImage};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrokeKind {
    /// Brightens by up to one stop.
    Dodge,
    /// Darkens by up to one stop.
    Burn,
    /// Blends toward a blurred copy of the current image.
    Smooth,
    /// Blends back toward the original image.
    BrushOut,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub kind: StrokeKind,
    /// Centre as fractions of the width and height.
    pub center: [f32; 2],
    /// Radius as a fraction of the shorter side.
    pub radius: f32,
    /// 0..1. 1 gives a hard edge, 0 a soft one.
    pub hardness: f32,
    /// 0..1. Coverage multiplier applied inside the mask.
    pub opacity: f32,
    /// 0..1. Overall strength of the operation, separate from opacity.
    pub overall_strength: f32,
}

impl Stroke {
    fn mask(&self, x: f32, y: f32, w: f32, h: f32) -> f32 {
        let cx = self.center[0] * w;
        let cy = self.center[1] * h;
        let r = (self.radius * w.min(h)).max(1e-3);
        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / r;
        let hard = self.hardness.clamp(0.0, 1.0);
        let edge = 1.0 - 0.9 * hard;
        1.0 - smoothstep(edge, 1.0, d)
    }
}

/// Applies strokes in order. `original` is the image before any local work, used by Brush Out.
pub fn apply(img: &mut LinearImage, original: &LinearImage, strokes: &[Stroke]) -> Result<(), EngineError> {
    for s in strokes {
        for v in [s.center[0], s.center[1], s.radius, s.hardness, s.opacity, s.overall_strength] {
            if !v.is_finite() {
                return Err(EngineError::NonFiniteParameter("stroke"));
            }
        }
    }
    let (w, h) = (img.width(), img.height());
    for s in strokes {
        let k = s.opacity.clamp(0.0, 1.0) * s.overall_strength.clamp(0.0, 1.0);
        if k == 0.0 {
            continue;
        }
        let blurred = if s.kind == StrokeKind::Smooth { Some(blur_all(img, (w.min(h) / 40).max(1))) } else { None };
        for y in 0..h {
            for x in 0..w {
                let a = s.mask(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32) * k;
                if a == 0.0 {
                    continue;
                }
                let i = (y * w + x) * 4;
                let px = &mut img.data_mut()[i..i + 4];
                match s.kind {
                    StrokeKind::Dodge | StrokeKind::Burn => {
                        let sign = if s.kind == StrokeKind::Dodge { 1.0 } else { -1.0 };
                        let g = (sign * a).exp2();
                        px[0] *= g;
                        px[1] *= g;
                        px[2] *= g;
                    }
                    StrokeKind::Smooth => {
                        let b = blurred.as_ref().unwrap();
                        for c in 0..3 {
                            px[c] += a * (b[(y * w + x) * 3 + c] - px[c]);
                        }
                    }
                    StrokeKind::BrushOut => {
                        let o = &original.data()[i..i + 3];
                        for c in 0..3 {
                            px[c] += a * (o[c] - px[c]);
                        }
                    }
                }
                px[0] = px[0].max(0.0);
                px[1] = px[1].max(0.0);
                px[2] = px[2].max(0.0);
            }
        }
    }
    Ok(())
}

/// Per-pixel RGB blur, three channels interleaved.
fn blur_all(img: &LinearImage, radius: usize) -> Vec<f32> {
    let (w, h) = (img.width(), img.height());
    let mut out = vec![0.0f32; w * h * 3];
    for c in 0..3 {
        let chan: Vec<f32> = img.data().chunks_exact(4).map(|p| p[c]).collect();
        let b = box_blur(&chan, w, h, radius);
        for (i, v) in b.into_iter().enumerate() {
            out[i * 3 + c] = v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive_exposure::luma;

    fn grey(v: u8) -> LinearImage {
        LinearImage::from_srgb8(16, 16, &vec![v; 16 * 16 * 4]).unwrap()
    }

    fn stroke(kind: StrokeKind) -> Stroke {
        Stroke { kind, center: [0.5, 0.5], radius: 0.3, hardness: 1.0, opacity: 1.0, overall_strength: 1.0 }
    }

    fn l_at(img: &LinearImage, x: usize, y: usize) -> f32 {
        let i = (y * img.width() + x) * 4;
        luma(&img.data()[i..i + 4])
    }

    #[test]
    fn no_strokes_is_bit_identical() {
        let original = grey(120);
        let mut img = grey(120);
        apply(&mut img, &original, &[]).unwrap();
        assert_eq!(img, original);
    }

    #[test]
    fn dodge_brightens_inside_the_mask_only() {
        let original = grey(100);
        let mut img = grey(100);
        apply(&mut img, &original, &[stroke(StrokeKind::Dodge)]).unwrap();
        assert!(l_at(&img, 8, 8) > l_at(&original, 8, 8));
        assert!((l_at(&img, 0, 0) - l_at(&original, 0, 0)).abs() < 1e-6);
    }

    #[test]
    fn burn_darkens_inside_the_mask() {
        let original = grey(180);
        let mut img = grey(180);
        apply(&mut img, &original, &[stroke(StrokeKind::Burn)]).unwrap();
        assert!(l_at(&img, 8, 8) < l_at(&original, 8, 8));
    }

    #[test]
    fn zero_overall_strength_is_identity() {
        let original = grey(100);
        let mut img = grey(100);
        let s = Stroke { overall_strength: 0.0, ..stroke(StrokeKind::Dodge) };
        apply(&mut img, &original, &[s]).unwrap();
        assert_eq!(img, original);
    }

    #[test]
    fn brush_out_returns_toward_the_original() {
        // Mask at the centre pixel is about 0.99, not 1, so compare against the dodge change.
        let original = grey(100);
        let mut dodged = grey(100);
        apply(&mut dodged, &original, &[stroke(StrokeKind::Dodge)]).unwrap();
        let mut img = dodged.clone();
        apply(&mut img, &original, &[stroke(StrokeKind::BrushOut)]).unwrap();
        let change = (l_at(&dodged, 8, 8) - l_at(&original, 8, 8)).abs();
        let residual = (l_at(&img, 8, 8) - l_at(&original, 8, 8)).abs();
        assert!(residual < 0.02 * change, "residual {residual} vs change {change}");
    }

    #[test]
    fn rejects_non_finite_stroke() {
        let original = grey(100);
        let mut img = grey(100);
        let s = Stroke { radius: f32::NAN, ..stroke(StrokeKind::Dodge) };
        assert!(apply(&mut img, &original, &[s]).is_err());
    }
}
