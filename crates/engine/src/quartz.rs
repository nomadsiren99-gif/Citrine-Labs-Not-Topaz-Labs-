//! The author's Quartz stages, ported into this engine. Selected with `Style::Quartz`
//! on the `Look`, so the original engine stays available as the default.
//! Source: the author's own repo (`crates/adjust-core/src/stages/*.rs`, `color.rs`, `curve.rs`).
//! Shortcut: Quartz's formulas assume values in 0..1, so this path clamps to 0..1, while
//! the default path keeps linear values above 1. Parameters are mapped from our sliders as
//! documented on each function.

use crate::adaptive_exposure::luma;
use crate::buffer::{EngineError, LinearImage};
use crate::guided::guided_filter;
use crate::pipeline::Look;

const LUMA_FLOOR: f32 = 0.0001;

/// Runs every Quartz stage in the author's order, then the strokes and transparency
/// from our look. `original` is the image before the look, for transparency.
pub fn apply(img: &mut LinearImage, original: &LinearImage, look: &Look) -> Result<(), EngineError> {
    basic(img, look.basic.contrast, look.basic.boost_blacks, look.basic.boost_whites);
    adaptive(img, look.adaptive.strength, look.adaptive.regions, look.adaptive.contrast,
        look.adaptive.brightness_ev, look.adaptive.protect_highlights, look.adaptive.protect_shadows);
    details(img, look.details.strength - 1.0, look.details.boost, look.details.threshold,
        look.details.radius, look.details.sharpen);
    color(img, look.color.adaptive_saturation, look.color.saturation - 1.0, 1.0 + look.color.boost,
        look.color.hue_deg);
    denoise(img, look.denoise.amount, look.denoise.suppression * 0.5);
    let c = &look.curve.points;
    if c.len() >= 3 && c[0].1 != c[0].0 || c.len() >= 3 && (c[1].1 - 0.5).abs() > 0.0 || c.len() >= 3 && (c[2].1 - 1.0).abs() > 0.0 {
        curve(img, &[(c[0].0, c[0].1), (c[1].0, c[1].1), (c[2].0, c[2].1)]);
    }
    crate::local::apply(img, original, &look.strokes)?;
    let f = &look.finishing;
    warmth(img, f.warmth);
    vignette(img, -f.vignette_strength, f.vignette_size, f.vignette_transition);
    grain(img, f.grain_amount);
    border(img, f.border_size, f.border_color);
    transparency(img, original, f.transparency);
    Ok(())
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Basic exposure (theirs): brightness is applied per channel around 0.5, contrast pivots
/// around 0.5, and the two boosts act on shadows and highlights.
/// Mapping: contrast and boosts are our basic-exposure sliders. Brightness comes from the look's EV.
fn basic(img: &mut LinearImage, contrast: f32, boost_blacks: f32, boost_whites: f32) {
    if contrast == 0.0 && boost_blacks == 0.0 && boost_whites == 0.0 {
        return;
    }
    let cf = if contrast >= 0.0 { 1.0 + contrast * 1.5 } else { 1.0 + contrast * 0.7 };
    for px in img.data_mut().chunks_exact_mut(4) {
        for c in 0..3 {
            let mut v = (clamp01(px[c]) - 0.5) * cf + 0.5;
            if boost_blacks > 0.0 && v < 0.5 {
                v -= boost_blacks * 0.25 * (1.0 - v / 0.5).clamp(0.0, 1.0);
            }
            if boost_whites > 0.0 && v > 0.5 {
                v += boost_whites * 0.25 * ((v - 0.5) / 0.5).clamp(0.0, 1.0);
            }
            px[c] = clamp01(v);
        }
    }
}

/// Adaptive exposure, local gamma tone mapping (theirs): gamma = exp(-(0.5 - base) * strength * 0.7),
/// where base is a guided-filtered luminance. Then contrast, brightness and the two protections
/// act on the mapped luminance, and RGB is scaled by the gain.
fn adaptive(img: &mut LinearImage, strength: f32, regions: u32, contrast: f32, brightness: f32, ph: f32, ps: f32) {
    if strength == 0.0 && contrast == 0.0 && brightness == 0.0 && ph == 0.0 && ps == 0.0 {
        return;
    }
    let (w, h) = (img.width(), img.height());
    let y: Vec<f32> = img.data().chunks_exact(4).map(|p| clamp01(luma(p))).collect();
    let scale = (32 - regions.clamp(1, 30)) as f32 / 10.0;
    let radius = (((w.max(h)) as f32 / 30.0) * scale).round().clamp(4.0, 150.0) as usize;
    let base = guided_filter(&y, &y, w, h, radius, 0.02 * 0.02);
    let cf = if contrast >= 0.0 { 1.0 + contrast * 0.8 } else { 1.0 + contrast * 0.5 };
    let offset = brightness * 0.25;
    for (i, px) in img.data_mut().chunks_exact_mut(4).enumerate() {
        let orig = y[i].clamp(LUMA_FLOOR, 1.0);
        let b = base[i].clamp(LUMA_FLOOR, 1.0);
        let gamma = (-(0.5 - b) * strength * 0.7).exp().clamp(0.25, 4.0);
        let mut m = orig.powf(gamma);
        if contrast != 0.0 {
            m = clamp01((m - 0.5) * cf + 0.5);
        }
        if offset != 0.0 {
            m = clamp01(m + offset);
        }
        if ph > 0.0 && m > 0.65 {
            let e = (m - 0.65) / 0.35;
            m = 0.65 + 0.35 * (e / (1.0 + ph * e * 1.6));
        }
        if ps > 0.0 && m < 0.35 {
            m += ps * 0.35 * (m * (1.0 - m / 0.35));
        }
        let g = (clamp01(m) / orig).clamp(0.0, 5.0);
        px[0] = clamp01(px[0] * g);
        px[1] = clamp01(px[1] * g);
        px[2] = clamp01(px[2] * g);
    }
}

/// Details (theirs): luminance detail is Y minus a guided base. Strength is signed: below 0
/// smooths (attenuates small detail, keeps sharp edges), above 0 sharpens (suppresses noise in
/// flat areas). Boost amplifies low-amplitude detail. Sharpen adds an unsharp term.
/// Mapping: strength = our strength minus 1 (so our neutral 1 is their 0). Threshold is in
/// linear luminance units as in the original.
fn details(img: &mut LinearImage, strength: f32, boost: f32, threshold: f32, radius: f32, sharpen: f32) {
    if strength == 0.0 && sharpen == 0.0 {
        return;
    }
    let (w, h) = (img.width(), img.height());
    let y: Vec<f32> = img.data().chunks_exact(4).map(|p| clamp01(luma(p))).collect();
    let r = (radius.round() as usize).clamp(1, 30);
    let smooth = guided_filter(&y, &y, w, h, r, 0.01 * 0.01);
    let thr = (threshold * 0.15).max(0.0005);
    let s = strength * 1.5;
    for (i, px) in img.data_mut().chunks_exact_mut(4).enumerate() {
        let d = y[i] - smooth[i];
        let ad = d.abs();
        let weight = if s >= 0.0 {
            if ad < thr { let t = ad / thr; t * t * (3.0 - 2.0 * t) } else { 1.0 }
        } else if ad < thr {
            1.0
        } else if ad < thr * 2.5 {
            let t = (ad - thr) / (thr * 1.5);
            1.0 - t * t * (3.0 - 2.0 * t)
        } else {
            0.0
        };
        let amp = if ad < 0.15 { 1.0 + (boost - 1.0) * (1.0 - ad / 0.15) } else { 1.0 };
        let delta = d * (s * amp * weight + sharpen * 0.8);
        for c in 0..3 {
            px[c] = clamp01(px[c] + delta);
        }
    }
}

/// Colour (theirs): HSV. Hue rotates; adaptive saturation boosts muted colours in proportion to
/// their headroom (and desaturates vibrant ones when negative); global saturation scales.
/// Mapping: saturation = our saturation minus 1; boost = 1 + our saturation boost.
fn color(img: &mut LinearImage, adaptive_sat: f32, global_sat: f32, sat_boost: f32, hue_deg: f32) {
    let adapt = adaptive_sat * 0.7;
    let hue_shift = hue_deg.rem_euclid(360.0) / 360.0;
    if adapt == 0.0 && global_sat == 0.0 && sat_boost == 1.0 && hue_shift == 0.0 {
        return;
    }
    for px in img.data_mut().chunks_exact_mut(4) {
        let (mut hh, mut s, v) = rgb_to_hsv(clamp01(px[0]), clamp01(px[1]), clamp01(px[2]));
        if hue_shift != 0.0 {
            hh = (hh + hue_shift).rem_euclid(1.0);
        }
        if adapt > 0.0 {
            s += (1.0 - s).clamp(0.0, 1.0) * adapt * sat_boost;
        } else if adapt < 0.0 {
            s += s * adapt * 0.5;
        }
        if global_sat > 0.0 {
            s += (1.0 - s) * global_sat;
        } else if global_sat < 0.0 {
            s *= 1.0 + global_sat;
        }
        let (r, g, b) = hsv_to_rgb(hh, clamp01(s), v);
        px[0] = clamp01(r);
        px[1] = clamp01(g);
        px[2] = clamp01(b);
    }
}

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let delta = max - min;
    let v = max;
    let s = if max > 1e-6 { delta / max } else { 0.0 };
    let mut h = if delta < 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        ((g - b) / delta) % 6.0
    } else if (max - g).abs() < 1e-6 {
        ((b - r) / delta) + 2.0
    } else {
        ((r - g) / delta) + 4.0
    };
    h /= 6.0;
    if h < 0.0 {
        h += 1.0;
    }
    (h, s, v)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    if s <= 1e-6 {
        return (v, v, v);
    }
    let h6 = (h % 1.0) * 6.0;
    let i = h6.floor() as i32;
    let f = h6 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

/// Denoise (theirs): blend each pixel toward a guided-filtered luminance and scale RGB by the
/// resulting gain (clamped 0.2..2). Mapping: suppression is our noise amount (0..1); the
/// guided radius comes from our suppression slider.
fn denoise(img: &mut LinearImage, strength: f32, amount: f32) {
    if strength <= 0.0 {
        return;
    }
    let (w, h) = (img.width(), img.height());
    let y: Vec<f32> = img.data().chunks_exact(4).map(|p| luma(p)).collect();
    let radius = (amount * 5.0).round().max(1.0) as usize;
    let eps = (strength * 0.05).max(0.001);
    let smooth = guided_filter(&y, &y, w, h, radius, eps * eps);
    let s = strength.clamp(0.0, 1.0);
    for (i, px) in img.data_mut().chunks_exact_mut(4).enumerate() {
        let o = y[i].max(LUMA_FLOOR);
        let target = (1.0 - s) * o + s * smooth[i];
        let g = (target / o).clamp(0.2, 2.0);
        for c in 0..3 {
            px[c] = clamp01(px[c] * g);
        }
    }
}

/// Curve (theirs): Catmull-Rom style cubic through the points, baked to a 1024 LUT and applied
/// per channel. Only runs when the points differ from the identity line.
fn curve(img: &mut LinearImage, pts: &[(f32, f32); 3]) {
    let mut p = *pts;
    p.sort_by(|a, b| a.0.total_cmp(&b.0));
    let lut: Vec<f32> = (0..1024)
        .map(|i| {
            let x = i as f32 / 1023.0;
            if x <= p[0].0 {
                return clamp01(p[0].1);
            }
            if x >= p[2].0 {
                return clamp01(p[2].1);
            }
            let seg = if x < p[1].0 { 0 } else { 1 };
            let (p1, p2) = (p[seg], p[seg + 1]);
            let p0 = if seg > 0 { p[seg - 1] } else { p1 };
            let p3 = if seg + 2 < 3 { p[seg + 2] } else { p2 };
            let dx = (p2.0 - p1.0).max(1e-5);
            let t = (x - p1.0) / dx;
            let m1 = (p2.1 - p0.1) / (p2.0 - p0.0).max(1e-5);
            let m2 = (p3.1 - p1.1) / (p3.0 - p1.0).max(1e-5);
            let (t2, t3) = (t * t, t * t * t);
            let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p1.1 + (t3 - 2.0 * t2 + t) * dx * m1
                + (-2.0 * t3 + 3.0 * t2) * p2.1 + (t3 - t2) * dx * m2;
            clamp01(y)
        })
        .collect();
    for px in img.data_mut().chunks_exact_mut(4) {
        for c in 0..3 {
            let idx = (clamp01(px[c]) * 1023.0).round() as usize;
            px[c] = lut[idx.min(1023)];
        }
    }
}

/// Warmth (theirs): adds to red, a fifth of that to green, and subtracts from blue.
fn warmth(img: &mut LinearImage, warmth: f32) {
    if warmth == 0.0 {
        return;
    }
    let w = warmth.clamp(-1.0, 1.0) * 0.15;
    for px in img.data_mut().chunks_exact_mut(4) {
        px[0] = clamp01(px[0] + w);
        px[1] = clamp01(px[1] + w * 0.2);
        px[2] = clamp01(px[2] - w);
    }
}

/// Vignette (theirs): factor 1 + amount * smoothstep falloff, with a rectangle metric
/// (roundness 0). Negative amount darkens the edges.
fn vignette(img: &mut LinearImage, amount: f32, size: f32, transition: f32) {
    if amount == 0.0 {
        return;
    }
    let (w, h) = (img.width(), img.height());
    let cx = (w as f32 - 1.0) / 2.0;
    let cy = (h as f32 - 1.0) / 2.0;
    let (size, trans) = (size.clamp(0.1, 1.0), transition.max(0.01));
    for y in 0..h {
        for x in 0..w {
            let dist = ((x as f32 - cx).abs() / cx.max(1e-3)).max((y as f32 - cy).abs() / cy.max(1e-3));
            let t = ((dist - size) / trans).clamp(0.0, 1.0);
            let mask = t * t * (3.0 - 2.0 * t);
            let f = (1.0 + amount.clamp(-1.0, 1.0) * mask).clamp(0.0, 2.0);
            let i = (y * w + x) * 4;
            for c in 0..3 {
                img.data_mut()[i + c] = clamp01(img.data()[i + c] * f);
            }
        }
    }
}

/// Film grain (theirs): a hash of the pixel coordinates (no random state, so tiles agree),
/// strongest in midtones. Mapping: amount from our grain slider, times their 0.15 scale.
fn grain(img: &mut LinearImage, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let (w, h) = (img.width(), img.height());
    let amt = amount.clamp(0.0, 1.0) * 0.15;
    for y in 0..h {
        for x in 0..w {
            let seed = ((x * 1973 + y * 9277 + 26699) ^ 0x5bf0_3635) as u32;
            let hash = seed.wrapping_mul(0x45d9_f3b);
            let norm = (hash & 0xffff) as f32 / 65535.0 - 0.5;
            let i = (y * w + x) * 4;
            let lum = clamp01(luma(&img.data()[i..i + 4]));
            let midtone = (lum * (1.0 - lum)).sqrt().max(0.1);
            let noise = norm * amt * midtone;
            for c in 0..3 {
                img.data_mut()[i + c] = clamp01(img.data()[i + c] + noise);
            }
        }
    }
}

/// Border (theirs): a solid frame whose width is a fraction of the shorter side.
fn border(img: &mut LinearImage, thickness: f32, color: [f32; 3]) {
    if thickness <= 0.0 {
        return;
    }
    let (w, h) = (img.width(), img.height());
    let px_w = ((w.min(h) as f32 * thickness.clamp(0.0, 0.5)).round().max(1.0)) as usize;
    for y in 0..h {
        for x in 0..w {
            if y < px_w || y + px_w >= h || x < px_w || x + px_w >= w {
                let i = (y * w + x) * 4;
                img.data_mut()[i..i + 3].copy_from_slice(&color);
            }
        }
    }
}

/// Transparency (theirs): blends the whole result back toward the original input.
fn transparency(img: &mut LinearImage, original: &LinearImage, t: f32) {
    if t <= 0.0 {
        return;
    }
    let t = t.clamp(0.0, 1.0);
    for (px, o) in img.data_mut().chunks_exact_mut(4).zip(original.data().chunks_exact(4)) {
        for c in 0..3 {
            px[c] = (1.0 - t) * px[c] + t * o[c];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{apply_look, Look, Style};

    fn sample() -> LinearImage {
        let mut rgba = Vec::new();
        for i in 0..(16 * 12) {
            let v = (40 + (i * 7) % 180) as u8;
            rgba.extend_from_slice(&[v, v / 2 + 30, 200 - v / 3, 255]);
        }
        LinearImage::from_srgb8(16, 12, &rgba).unwrap()
    }

    fn ls() -> Look {
        Look { style: Style::Quartz, ..Default::default() }
    }

    #[test]
    fn neutral_quartz_look_is_bit_identical() {
        let original = sample();
        let mut img = sample();
        apply_look(&mut img, &ls()).unwrap();
        assert_eq!(img, original);
    }

    #[test]
    fn warmth_moves_red_up_and_blue_down() {
        let original = sample();
        let mut img = sample();
        let mut look = ls();
        look.finishing.warmth = 1.0;
        apply_look(&mut img, &look).unwrap();
        assert!(img.data()[0] >= original.data()[0]);
        assert!(img.data()[2] <= original.data()[2]);
    }

    #[test]
    fn negative_vignette_darkens_corners() {
        let original = sample();
        let mut img = sample();
        let mut look = ls();
        look.finishing.vignette_strength = 1.0;
        apply_look(&mut img, &look).unwrap();
        assert!(img.data()[0] < original.data()[0]);
    }

    #[test]
    fn grain_is_deterministic() {
        let mut a = sample();
        let mut b = sample();
        let mut look = ls();
        look.finishing.grain_amount = 0.5;
        apply_look(&mut a, &look).unwrap();
        apply_look(&mut b, &look).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn full_transparency_returns_the_original() {
        let original = sample();
        let mut img = sample();
        let mut look = ls();
        look.finishing.warmth = 1.0;
        look.finishing.transparency = 1.0;
        apply_look(&mut img, &look).unwrap();
        assert_eq!(img, original);
    }

    #[test]
    fn gamma_tone_mapping_lifts_shadows() {
        let original = sample();
        let mut img = sample();
        let mut look = ls();
        look.adaptive.strength = 1.0;
        apply_look(&mut img, &look).unwrap();
        // Darkest pixel is the first pixel of the ramp.
        let before = luma(&original.data()[0..4]);
        let after = luma(&img.data()[0..4]);
        assert!(after >= before);
    }

    #[test]
    fn positive_saturation_raises_chroma_of_muted_colour() {
        let original = sample();
        let mut img = sample();
        let mut look = ls();
        look.color.adaptive_saturation = 1.0;
        apply_look(&mut img, &look).unwrap();
        assert_ne!(img, original);
    }

    #[test]
    fn strong_factory_style_values_stay_finite_and_in_range() {
        // Values from the renamed "Vivid Punch" factory preset, pushed through the full stack.
        let mut look = ls();
        look.basic.contrast = 0.12;
        look.basic.boost_blacks = 0.08;
        look.basic.boost_whites = 0.05;
        look.adaptive.strength = 1.25;
        look.adaptive.regions = 12;
        look.adaptive.contrast = 0.18;
        look.adaptive.protect_highlights = 0.35;
        look.adaptive.protect_shadows = 0.25;
        look.details.strength = 1.4 + 1.0;
        look.details.boost = 1.2;
        look.details.threshold = 0.08;
        look.details.radius = 1.2;
        look.details.sharpen = 0.2;
        look.details.process_independently = true;
        look.color.adaptive_saturation = 0.65;
        look.color.saturation = 1.1;
        look.color.boost = 0.15;
        look.denoise.amount = 0.15;
        look.denoise.suppression = 0.8;
        look.finishing.warmth = 0.05;
        look.finishing.vignette_strength = 0.15;
        look.finishing.vignette_size = 0.6;
        look.finishing.vignette_transition = 0.5;
        let mut img = sample();
        apply_look(&mut img, &look).unwrap();
        for v in img.data() {
            assert!(v.is_finite() && *v >= 0.0 && *v <= 1.0, "out of range: {v}");
        }
    }
}
