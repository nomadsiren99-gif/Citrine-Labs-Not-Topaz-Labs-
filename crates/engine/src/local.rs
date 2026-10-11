//! Local brush tools v1 (spec Milestone 8). Each stroke is a soft circular mask with a kind:
//! Dodge, Burn, Smooth or Brush Out. Strokes are applied in order. Independent proposal, not a
//! reconstruction of Topaz. A brush UI is not built yet; masks come from parameters.

use crate::adaptive_exposure::{box_blur, luma, smoothstep};
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
    /// Removes its coverage from every earlier stroke. Applies no change of its own.
    Erase,
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
    /// 0..1. Pulls the mask toward the image's own edges (edge-preserving), so a stroke does not
    /// spill across a strong boundary. 0 keeps the plain circular mask.
    pub edge_aware: f32,
}

impl Stroke {
    fn mask(&self, x: f32, y: f32, w: f32, h: f32) -> f32 {
        let cx = self.center[0] * w;
        let cy = self.center[1] * h;
        let r = (self.radius * w.min(h)).max(1e-3);
        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / r;
        // Hardness 1 starts the falloff just inside the radius (sharp edge); 0 starts it at the
        // centre (widest falloff). The 0.98 cap keeps a small band so the edge is never a step.
        let hard = self.hardness.clamp(0.0, 1.0);
        let edge = hard.min(0.98);
        1.0 - smoothstep(edge, 1.0, d)
    }
}

/// Coverage of every stroke that is still active, in order, after erasers have cut earlier ones.
/// Each mask already includes the stroke's opacity and strength. Used by `apply` and by the
/// mask view, so the overlay shows exactly what is applied.
fn active_masks(img: &LinearImage, strokes: &[Stroke]) -> Vec<(usize, Vec<f32>)> {
    let (w, h) = (img.width(), img.height());
    let source = img.clone();
    let mut masks: Vec<(usize, Vec<f32>)> = Vec::new();
    for (idx, s) in strokes.iter().enumerate() {
        let k = s.opacity.clamp(0.0, 1.0) * s.overall_strength.clamp(0.0, 1.0);
        if k == 0.0 {
            continue;
        }
        let mut mask: Vec<f32> = (0..w * h)
            .map(|i| s.mask((i % w) as f32 + 0.5, (i / w) as f32 + 0.5, w as f32, h as f32))
            .collect();
        if s.edge_aware > 0.0 {
            refine_to_edges(&mut mask, &source, s.edge_aware.clamp(0.0, 1.0), w, h);
        }
        if s.kind == StrokeKind::Erase {
            for (_, earlier) in masks.iter_mut() {
                // Subtract coverage: where the eraser fully covers a stroke, that stroke is gone.
                for (e, m) in earlier.iter_mut().zip(&mask) {
                    *e = (*e - m * k).max(0.0);
                }
            }
        } else {
            masks.push((idx, mask.iter().map(|m| m * k).collect()));
        }
    }
    masks
}

/// Combined coverage of all active strokes, 0..1 per pixel, for the mask view.
pub fn coverage(img: &LinearImage, strokes: &[Stroke]) -> Vec<f32> {
    let mut out = vec![0.0f32; img.width() * img.height()];
    for (_, mask) in active_masks(img, strokes) {
        for (o, m) in out.iter_mut().zip(&mask) {
            *o = o.max(*m);
        }
    }
    out
}

/// Applies strokes in order. `original` is the image before any local work, used by Brush Out.
pub fn apply(img: &mut LinearImage, original: &LinearImage, strokes: &[Stroke]) -> Result<(), EngineError> {
    for s in strokes {
        for v in [s.center[0], s.center[1], s.radius, s.hardness, s.opacity, s.overall_strength, s.edge_aware] {
            if !v.is_finite() {
                return Err(EngineError::NonFiniteParameter("stroke"));
            }
        }
    }
    let (w, h) = (img.width(), img.height());
    let masks = active_masks(img, strokes);
    for (idx, mask) in &masks {
        let s = &strokes[*idx];
        let blurred = if s.kind == StrokeKind::Smooth { Some(blur_all(img, (w.min(h) / 40).max(1))) } else { None };
        for y in 0..h {
            for x in 0..w {
                let a = mask[y * w + x];
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
                    StrokeKind::Erase => {}
                }
                px[0] = px[0].max(0.0);
                px[1] = px[1].max(0.0);
                px[2] = px[2].max(0.0);
            }
        }
    }
    Ok(())
}

/// Bilateral similarity: each pixel's coverage is weighted by how close its brightness is to the
/// stroke's core, so a stroke does not spill across a strong tonal boundary. `amount` 0 keeps the
/// mask; 1 applies the full weight.
fn refine_to_edges(mask: &mut [f32], img: &LinearImage, amount: f32, w: usize, h: usize) {
    const SIGMA: f32 = 0.15; // luma tolerance, linear light. Proposed.
    let lum: Vec<f32> = img.data().chunks_exact(4).map(luma).collect();
    // Reference: mean brightness of the stroke core, or the centre pixel if the core is empty.
    let core: Vec<f32> = mask.iter().zip(&lum).filter(|(m, _)| **m >= 0.9).map(|(_, l)| *l).collect();
    let reference = if core.is_empty() { lum[(h / 2) * w + w / 2] } else { core.iter().sum::<f32>() / core.len() as f32 };
    for (m, l) in mask.iter_mut().zip(&lum) {
        let d = (l - reference) / SIGMA;
        let similarity = (-0.5 * d * d).exp();
        *m *= (1.0 - amount) + amount * similarity;
    }
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
        Stroke { kind, center: [0.5, 0.5], radius: 0.3, hardness: 1.0, opacity: 1.0, overall_strength: 1.0, edge_aware: 0.0 }
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

#[cfg(test)]
mod gate5_tests {
    //! Gate 5: masks are deterministic, hardness sets the edge width, and Smooth removes texture
    //! only where the stroke covers it.
    use super::*;
    use crate::adaptive_exposure::luma;

    fn grey(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> LinearImage {
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let v = f(x, y);
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        LinearImage::from_srgb8(w, h, &rgba).unwrap()
    }

    #[test]
    fn hard_edge_is_sharp_and_soft_edge_is_wide() {
        let s = |hardness| Stroke { kind: StrokeKind::Dodge, center: [0.5, 0.5], radius: 0.25, hardness, opacity: 1.0, overall_strength: 1.0, edge_aware: 0.0 };
        let (w, h) = (100.0f32, 100.0f32);
        // Radius 25 px. Sample 2 px inside and 2 px outside the edge, then at the centre.
        let inside = |s: &Stroke| s.mask(50.0 + 23.0, 50.0, w, h);
        let outside = |s: &Stroke| s.mask(50.0 + 27.0, 50.0, w, h);
        // Edge at 25 px: 23 px is inside, 27 px is outside.
        let hard = s(1.0);
        let soft = s(0.0);
        assert!(inside(&hard) > 0.99 && outside(&hard) < 0.01, "hard edge not sharp");
        // Soft: the falloff is gradual, so a point 18 px out (d = 0.72) is neither fully in nor out.
        let mid = |s: &Stroke| s.mask(50.0 + 18.0, 50.0, w, h);
        assert!(mid(&soft) > 0.05 && mid(&soft) < 0.95, "soft edge not gradual: {}", mid(&soft));
        assert!(mid(&hard) > 0.99, "hard edge should be full at 18 px");
        assert!((s(1.0).mask(50.0, 50.0, w, h) - 1.0).abs() < 1e-6, "centre must be full");
    }

    #[test]
    fn smooth_removes_texture_inside_the_stroke_only() {
        let img = grey(96, 96, |x, y| if (x + y) % 2 == 0 { 100 } else { 130 });
        let mut out = img.clone();
        let stroke = Stroke { kind: StrokeKind::Smooth, center: [0.25, 0.5], radius: 0.2, hardness: 1.0, opacity: 1.0, overall_strength: 1.0, edge_aware: 0.0 };
        apply(&mut out, &img, &[stroke]).unwrap();
        let contrast = |i: &LinearImage, x: usize| (luma(&i.data()[(48 * 96 + x) * 4..]) - luma(&i.data()[(48 * 96 + x + 1) * 4..])).abs();
        assert!(contrast(&out, 24) < 0.5 * contrast(&img, 24), "texture inside the stroke survived");
        assert!((contrast(&out, 80) - contrast(&img, 80)).abs() < 1e-6, "texture outside the stroke changed");
    }

    #[test]
    fn same_strokes_give_the_same_image() {
        let img = grey(64, 64, |x, y| ((x * 3 + y * 5) % 200) as u8 + 20);
        let strokes = [
            Stroke { kind: StrokeKind::Burn, center: [0.3, 0.3], radius: 0.2, hardness: 0.4, opacity: 0.8, overall_strength: 0.6, edge_aware: 0.0 },
            Stroke { kind: StrokeKind::Smooth, center: [0.7, 0.6], radius: 0.25, hardness: 0.2, opacity: 1.0, overall_strength: 0.5, edge_aware: 0.0 },
        ];
        let mut a = img.clone();
        let mut b = img.clone();
        apply(&mut a, &img, &strokes).unwrap();
        apply(&mut b, &img, &strokes).unwrap();
        assert_eq!(a, b);
    }
}

#[cfg(test)]
mod edge_aware_tests {
    //! Edge-aware masks keep a stroke on its own side of a strong boundary.
    use super::*;
    use crate::adaptive_exposure::luma;

    /// 96x96 image: left 48 px dark (sRGB 60), right 48 px bright (sRGB 200).
    fn step() -> LinearImage {
        let mut rgba = Vec::new();
        for _ in 0..96 {
            for x in 0..96 {
                let v = if x < 48 { 60u8 } else { 200u8 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        LinearImage::from_srgb8(96, 96, &rgba).unwrap()
    }

    /// Total brightening (in log2 luma) on the bright side, pixels 49..80.
    fn spill(img: &LinearImage, before: &LinearImage) -> f32 {
        (49..80)
            .map(|x| (luma(&img.data()[(48 * 96 + x) * 4..]) / luma(&before.data()[(48 * 96 + x) * 4..])).log2().max(0.0))
            .sum()
    }

    #[test]
    fn edge_aware_stroke_stays_on_its_side() {
        // Stroke centred on the dark side, wide enough to spill across the edge at x = 48.
        let base = step();
        let stroke = |edge_aware| Stroke {
            kind: StrokeKind::Dodge,
            center: [0.3, 0.5],
            radius: 0.4,
            hardness: 0.0,
            opacity: 1.0,
            overall_strength: 1.0,
            edge_aware,
        };
        let mut plain = base.clone();
        apply(&mut plain, &base, &[stroke(0.0)]).unwrap();
        let mut refined = base.clone();
        apply(&mut refined, &base, &[stroke(1.0)]).unwrap();
        let (p, r) = (spill(&plain, &base), spill(&refined, &base));
        eprintln!("edge-aware spill: plain {p:.4}, refined {r:.4}");
        assert!(p > 0.05, "setup: the plain stroke should spill across the edge ({p})");
        assert!(r < 0.5 * p, "edge-aware did not keep the stroke on its side: {r:.4} vs {p:.4}");
    }
}

#[cfg(test)]
mod eraser_tests {
    //! Eraser: subtracts its coverage from earlier strokes, and an erased stroke leaves no effect.
    use super::*;

    fn grey() -> LinearImage {
        let mut rgba = Vec::new();
        for _ in 0..64 * 64 {
            rgba.extend_from_slice(&[120, 120, 120, 255]);
        }
        LinearImage::from_srgb8(64, 64, &rgba).unwrap()
    }

    fn dodge(center: [f32; 2], radius: f32) -> Stroke {
        Stroke { kind: StrokeKind::Dodge, center, radius, hardness: 1.0, opacity: 1.0, overall_strength: 1.0, edge_aware: 0.0 }
    }

    #[test]
    fn erasing_the_whole_stroke_restores_the_image() {
        let base = grey();
        let erase = Stroke { kind: StrokeKind::Erase, ..dodge([0.5, 0.5], 0.4) };
        let mut out = base.clone();
        apply(&mut out, &base, &[dodge([0.5, 0.5], 0.4), erase]).unwrap();
        assert_eq!(out, base, "a full eraser over the stroke should leave no trace");
    }

    #[test]
    fn eraser_cuts_only_its_own_region() {
        let base = grey();
        let erase = Stroke { kind: StrokeKind::Erase, ..dodge([0.2, 0.5], 0.2) };
        let mut out = base.clone();
        apply(&mut out, &base, &[dodge([0.5, 0.5], 0.4), erase]).unwrap();
        let lum = |x: usize| luma(&out.data()[(32 * 64 + x) * 4..]);
        let at = |x: usize| luma(&base.data()[(32 * 64 + x) * 4..]);
        assert!((lum(13) - at(13)).abs() < 1e-6, "erased area should be back to the input");
        assert!(lum(32) > at(32) * 1.05, "area outside the eraser should keep the dodge");
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    #[test]
    fn coverage_shows_the_stroke_and_drops_after_an_eraser() {
        let img = LinearImage::new(32, 32, vec![0.5; 32 * 32 * 4]).unwrap();
        let dodge = Stroke { kind: StrokeKind::Dodge, center: [0.5, 0.5], radius: 0.3, hardness: 1.0, opacity: 1.0, overall_strength: 1.0, edge_aware: 0.0 };
        let shown = coverage(&img, &[dodge]);
        assert!(shown[16 * 32 + 16] > 0.99 && shown[0] == 0.0, "coverage should be full in the stroke only");
        let erase = Stroke { kind: StrokeKind::Erase, ..dodge };
        let cut = coverage(&img, &[dodge, erase]);
        assert!(cut.iter().all(|v| *v < 1e-6), "a full eraser should clear the overlay");
    }
}
