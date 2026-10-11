//! Spec section 13, functional tests: every control has an explicit neutral value that returns
//! the input unchanged, and a moderate value that changes pixels in the expected direction.

use engine::adaptive_exposure::AdaptiveExposure;
use engine::basic_exposure::BasicExposure;
use engine::color_adjust::ColorAdjust;
use engine::curve::Curve;
use engine::details::Details;
use engine::finishing::Finishing;
use engine::local::{Stroke, StrokeKind};
use engine::noise::Denoise;
use engine::{apply_look, LinearImage, Look};

/// 96x64 photo-like test scene: a left-to-right gradient, texture, and a warm-to-cool split.
fn scene() -> LinearImage {
    let (w, h) = (96usize, 64usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let t = ((x * 13 + y * 7) % 11) as f32 / 11.0;
            // Reaches near 250 so highlight controls (diffusion, whites) have something to act on.
            let base = 30.0 + 220.0 * x as f32 / (w - 1) as f32;
            let r = (base + 12.0 * t).clamp(0.0, 255.0) as u8;
            let g = (base * 0.9 + 6.0 * t).clamp(0.0, 255.0) as u8;
            let b = (base * 0.75 + 4.0 * (1.0 - t)).clamp(0.0, 255.0) as u8;
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    LinearImage::from_srgb8(w, h, &rgba).unwrap()
}

fn render(look: &Look) -> LinearImage {
    let mut img = scene();
    apply_look(&mut img, look).unwrap();
    img
}

fn mean_luma(img: &LinearImage) -> f32 {
    let d = img.data();
    d.chunks_exact(4).map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).sum::<f32>() / (d.len() / 4) as f32
}

/// Mean absolute change per channel, against the input scene.
fn change(img: &LinearImage) -> f32 {
    let before = scene();
    img.data().iter().zip(before.data()).map(|(a, b)| (a - b).abs()).sum::<f32>() / img.data().len() as f32
}

fn assert_neutral(name: &str, look: Look) {
    let out = render(&look);
    assert_eq!(out, scene(), "{name}: neutral setting changed pixels");
}

fn assert_moves(name: &str, look: Look, min_change: f32) {
    let c = change(&render(&look));
    assert!(c > min_change, "{name}: change {c:.5} below {min_change}");
}

#[test]
fn adaptive_exposure_controls() {
    assert_neutral("adaptive strength 0", Look { adaptive: AdaptiveExposure { strength: 0.0, ..Default::default() }, ..Default::default() });
    assert_moves("adaptive strength 1", Look { adaptive: AdaptiveExposure { strength: 1.0, regions: 4, ..Default::default() }, ..Default::default() }, 1e-3);
    // Brightness is an EV offset on linear light: +1 EV doubles the mean.
    let base = mean_luma(&render(&Look::default()));
    let up = mean_luma(&render(&Look { adaptive: AdaptiveExposure { brightness_ev: 1.0, ..Default::default() }, ..Default::default() }));
    assert!((up / base - 2.0).abs() < 0.05, "brightness +1 EV should double linear luma: {base} -> {up}");
    assert_moves("adaptive contrast", Look { adaptive: AdaptiveExposure { strength: 1.0, regions: 4, contrast: 0.8, ..Default::default() }, ..Default::default() }, 1e-3);
}

#[test]
fn basic_exposure_controls() {
    assert_neutral("basic neutral", Look { basic: BasicExposure::default(), ..Default::default() });
    assert_moves("basic contrast", Look { basic: BasicExposure { contrast: 0.8, ..Default::default() }, ..Default::default() }, 1e-3);
    assert_moves("basic boost blacks", Look { basic: BasicExposure { boost_blacks: 0.8, ..Default::default() }, ..Default::default() }, 1e-3);
    assert_moves("basic boost whites", Look { basic: BasicExposure { boost_whites: 0.8, ..Default::default() }, ..Default::default() }, 1e-4);
}

#[test]
fn details_controls() {
    assert_neutral("details neutral", Look { details: Details::default(), ..Default::default() });
    // Sharpen is edge-gated by design: texture at the image's own noise level is left alone, so
    // this uses a clear edge.
    let edge = {
        let (w, h) = (64usize, 4usize);
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let v = if x < 32 { 90u8 } else { 170u8 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        LinearImage::from_srgb8(w, h, &rgba).unwrap()
    };
    let mut sharp = edge.clone();
    apply_look(&mut sharp, &Look { details: Details { sharpen: 1.0, radius: 2.0, ..Default::default() }, ..Default::default() }).unwrap();
    let edge_change: f32 = sharp.data().iter().zip(edge.data()).map(|(a, b)| (a - b).abs()).sum();
    assert!(edge_change > 1e-3, "sharpen did not change a clear edge: {edge_change}");
    // Boost acts on weak, coherent detail: a small step between two flat areas (not noise).
    let weak = {
        let mut rgba = Vec::new();
        for _ in 0..4 {
            for x in 0..64 {
                let v = if x < 32 { 100u8 } else { 110u8 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        LinearImage::from_srgb8(64, 4, &rgba).unwrap()
    };
    let mut boosted = weak.clone();
    apply_look(&mut boosted, &Look { details: Details { boost: 2.0, radius: 2.0, ..Default::default() }, ..Default::default() }).unwrap();
    let weak_change: f32 = boosted.data().iter().zip(weak.data()).map(|(a, b)| (a - b).abs()).sum();
    assert!(weak_change > 1e-3, "boost did not change a weak step: {weak_change}");
    assert_moves("details strength below 1 (smoothing)", Look { details: Details { strength: 0.0, boost: 0.0, radius: 2.0, ..Default::default() }, ..Default::default() }, 1e-4);
}

#[test]
fn colour_controls() {
    assert_neutral("colour neutral", Look { color: ColorAdjust::default(), ..Default::default() });
    let grey = Look { color: ColorAdjust { saturation: 0.0, ..Default::default() }, ..Default::default() };
    assert_moves("saturation 0 removes colour", grey.clone(), 1e-3);
    let out = render(&grey);
    let d = out.data();
    let spread: f32 = d.chunks_exact(4).map(|p| (p[0] - p[2]).abs()).sum::<f32>() / (d.len() / 4) as f32;
    assert!(spread < 0.01, "saturation 0 left colour, mean R-B {spread}");
    assert_moves("hue rotation", Look { color: ColorAdjust { hue_deg: 90.0, ..Default::default() }, ..Default::default() }, 1e-3);
    assert_moves("adaptive saturation", Look { color: ColorAdjust { adaptive_saturation: 1.0, ..Default::default() }, ..Default::default() }, 1e-4);
    assert_moves("saturation boost", Look { color: ColorAdjust { boost: 1.0, ..Default::default() }, ..Default::default() }, 1e-4);
}

#[test]
fn noise_controls() {
    assert_neutral("denoise amount 0", Look { denoise: Denoise { amount: 0.0, ..Default::default() }, ..Default::default() });
    assert_moves("denoise amount 1", Look { denoise: Denoise { amount: 1.0, suppression: 1.0 }, ..Default::default() }, 1e-4);
}

#[test]
fn curve_controls() {
    assert_neutral("identity curve", Look { curve: Curve::default(), ..Default::default() });
    assert_moves("darkening curve", Look { curve: Curve { points: vec![(0.0, 0.0), (0.5, 0.25), (1.0, 1.0)] }, ..Default::default() }, 1e-3);
}

#[test]
fn local_tools() {
    let stroke = |kind| Stroke { kind, center: [0.5, 0.5], radius: 0.3, hardness: 0.5, opacity: 1.0, overall_strength: 1.0, edge_aware: 0.0 };
    assert_neutral("zero-strength dodge", Look { strokes: vec![Stroke { overall_strength: 0.0, ..stroke(StrokeKind::Dodge) }], ..Default::default() });
    assert_moves("dodge", Look { strokes: vec![stroke(StrokeKind::Dodge)], ..Default::default() }, 1e-3);
    assert_moves("burn", Look { strokes: vec![stroke(StrokeKind::Burn)], ..Default::default() }, 1e-3);
    assert_moves("smooth", Look { strokes: vec![stroke(StrokeKind::Smooth)], ..Default::default() }, 1e-4);
}

#[test]
fn finishing_controls() {
    assert_neutral("finishing neutral", Look { finishing: Finishing::default(), ..Default::default() });
    assert_moves("warmth +", Look { finishing: Finishing { warmth: 0.6, ..Default::default() }, ..Default::default() }, 1e-3);
    assert_moves("diffusion", Look { finishing: Finishing { diffusion: 0.8, ..Default::default() }, ..Default::default() }, 1e-4);
    assert_moves("vignette", Look { finishing: Finishing { vignette_strength: 0.8, ..Default::default() }, ..Default::default() }, 1e-3);
    assert_moves("grain", Look { finishing: Finishing { grain_amount: 0.5, ..Default::default() }, ..Default::default() }, 1e-3);
    assert_moves("border", Look { finishing: Finishing { border_size: 0.05, ..Default::default() }, ..Default::default() }, 1e-3);
    assert_moves("tone", Look { finishing: Finishing { tone_strength: 0.8, tone_colors: [[1.0, 0.6, 0.4]; 4], ..Default::default() }, ..Default::default() }, 1e-3);
    // Transparency 1 returns the input exactly, whatever else is set.
    let t = Look { finishing: Finishing { warmth: 0.6, vignette_strength: 0.8, transparency: 1.0, ..Default::default() }, ..Default::default() };
    assert_eq!(render(&t), scene(), "transparency 1 must return the original");
}

#[test]
fn new_finishing_controls() {
    // Softness lowers fine texture (edge-preserving blur), without changing the mean much.
    let tex = |img: &LinearImage| {
        let d = img.data();
        let l: Vec<f32> = d.chunks_exact(4).map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).collect();
        let (w, h) = (img.width(), img.height());
        let mut sum = 0.0f32;
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                sum += (l[y * w + x] - l[y * w + x + 1]).abs();
            }
        }
        sum
    };
    let base = scene();
    let mut soft = scene();
    apply_look(&mut soft, &Look { finishing: Finishing { diffusion_softness: 1.0, ..Default::default() }, ..Default::default() }).unwrap();
    assert!(tex(&soft) < 0.97 * tex(&base), "softness did not reduce texture: {} vs {}", tex(&soft), tex(&base));

    // Curvature changes the vignette falloff; zero is the original curve.
    let v0 = render(&Look { finishing: Finishing { vignette_strength: 0.8, ..Default::default() }, ..Default::default() });
    let v1 = render(&Look { finishing: Finishing { vignette_strength: 0.8, vignette_curvature: 0.8, ..Default::default() }, ..Default::default() });
    let diff: f32 = v0.data().iter().zip(v1.data()).map(|(a, b)| (a - b).abs()).sum();
    assert!(diff > 1e-3, "curvature had no effect");

    // Moving the tone anchor moves the tint: a red tint on anchor 0 changes dark pixels only.
    let mut tone = [[1.0f32; 3]; 4];
    tone[0] = [1.0, 0.4, 0.4];
    let look_at = |pos: [f32; 4]| Look { finishing: Finishing { tone_colors: tone, tone_strength: 1.0, tone_positions: pos, ..Default::default() }, ..Default::default() };
    let low = render(&look_at([0.1, 0.35, 0.6, 0.9]));
    let high = render(&look_at([0.8, 0.85, 0.9, 0.95]));
    let shift = |img: &LinearImage| {
        let d = img.data();
        d.chunks_exact(4).map(|p| (p[0] - p[1]).abs()).sum::<f32>()
    };
    assert!((shift(&low) - shift(&high)).abs() > 1e-3, "tone positions had no effect");
}
