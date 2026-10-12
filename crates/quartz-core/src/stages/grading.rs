use crate::color::hsv_to_rgb;
use crate::image_buffer::RgbImageF32;
use crate::params::{ColorGradingParams, ColorWheel};
use rayon::prelude::*;
use super::basic_correction::smoothstep;

/// Three-zone colour grading. Each pixel's luminance gives its weight in shadows, midtones and
/// highlights; each zone adds its wheel's colour (chroma with no luminance change) and luminance.
pub fn apply_color_grading(img: &mut RgbImageF32, p: &ColorGradingParams) {
    if !p.enabled || is_neutral(p) {
        return;
    }
    let mid = (0.5 + p.balance * 0.3).clamp(0.1, 0.9);
    let width = 0.1 + p.blending.clamp(0.0, 1.0) * 0.5;
    let zones = [zone(&p.shadows), zone(&p.midtones), zone(&p.highlights)];
    img.r
        .par_iter_mut()
        .zip(img.g.par_iter_mut())
        .zip(img.b.par_iter_mut())
        .for_each(|((r, g), b)| {
            let (nr, ng, nb) = grade_pixel(*r, *g, *b, mid, width, &zones);
            *r = nr;
            *g = ng;
            *b = nb;
        });
}

fn is_neutral(p: &ColorGradingParams) -> bool {
    [&p.shadows, &p.midtones, &p.highlights]
        .iter()
        .all(|w| w.sat == 0.0 && w.lum == 0.0)
}

/// A wheel's colour offset (zero luminance) and luminance offset.
fn zone(w: &ColorWheel) -> ([f32; 3], f32) {
    let (tr, tg, tb) = hsv_to_rgb((w.hue / 360.0).rem_euclid(1.0), 1.0, 1.0);
    let lum = 0.2126 * tr + 0.7152 * tg + 0.0722 * tb;
    let chroma = w.sat.clamp(0.0, 1.0) * 0.25;
    ([(tr - lum) * chroma, (tg - lum) * chroma, (tb - lum) * chroma], w.lum.clamp(-1.0, 1.0) * 0.25)
}

/// One pixel. Also used as the reference for the GPU shader.
pub fn grade_pixel(r: f32, g: f32, b: f32, mid: f32, width: f32, zones: &[([f32; 3], f32); 3]) -> (f32, f32, f32) {
    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let ws = 1.0 - smoothstep(mid - width, mid, lum);
    let wh = smoothstep(mid, mid + width, lum);
    let wm = (1.0 - ws - wh).clamp(0.0, 1.0);
    let weights = [ws, wm, wh];
    let (mut r, mut g, mut b) = (r, g, b);
    for (zone, w) in zones.iter().zip(weights) {
        let (c, l) = zone;
        r += (c[0] + l) * w;
        g += (c[1] + l) * w;
        b += (c[2] + l) * w;
    }
    (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_wheels_change_nothing() {
        let p = ColorGradingParams::default();
        let img = RgbImageF32::new(1, 1);
        let mut out = img.clone();
        apply_color_grading(&mut out, &p);
        assert_eq!(out.r, img.r);
    }

    #[test]
    fn a_shadow_wheel_tints_shadows_not_highlights() {
        let zones = [zone(&ColorWheel { hue: 210.0, sat: 1.0, lum: 0.0 }), zone(&ColorWheel::default()), zone(&ColorWheel::default())];
        let (_, _, b_dark) = grade_pixel(0.05, 0.05, 0.05, 0.5, 0.35, &zones);
        assert!(b_dark > 0.05, "a blue shadow wheel should lift blue in shadows");
        let (r_bright, g_bright, b_bright) = grade_pixel(0.95, 0.95, 0.95, 0.5, 0.35, &zones);
        assert!((r_bright - 0.95).abs() < 1e-6 && (g_bright - 0.95).abs() < 1e-6 && (b_bright - 0.95).abs() < 1e-6);
    }
}
