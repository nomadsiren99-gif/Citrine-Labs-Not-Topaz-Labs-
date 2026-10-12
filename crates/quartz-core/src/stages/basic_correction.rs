use crate::color::srgb_to_linear;
use crate::image_buffer::RgbImageF32;
use crate::params::BasicCorrectionParams;
use rayon::prelude::*;

/// Camera Raw style basic correction, per channel on 0..1 values. Exposure is applied in linear
/// light; the other sliders are tone maps around the grey and the ends of the range.
/// These are this app's own curves, not Adobe's.
pub fn apply_basic_correction(img: &mut RgbImageF32, p: &BasicCorrectionParams) {
    if !p.enabled || is_neutral(p) {
        return;
    }
    img.r
        .par_iter_mut()
        .zip(img.g.par_iter_mut())
        .zip(img.b.par_iter_mut())
        .for_each(|((r, g), b)| {
            *r = basic_map(*r, p);
            *g = basic_map(*g, p);
            *b = basic_map(*b, p);
        });
}

fn is_neutral(p: &BasicCorrectionParams) -> bool {
    p.exposure == 0.0 && p.contrast == 0.0 && p.highlights == 0.0 && p.shadows == 0.0 && p.whites == 0.0 && p.blacks == 0.0
}

/// The per-channel map. Also used as the reference for the GPU shader.
pub fn basic_map(v: f32, p: &BasicCorrectionParams) -> f32 {
    let mut x = v;
    if p.exposure != 0.0 {
        x = linear_to_srgb_clamped(srgb_to_linear(x) * 2f32.powf(p.exposure));
    }
    if p.contrast > 0.0 {
        x += (smoothstep(0.0, 1.0, x) - x) * p.contrast;
    } else if p.contrast < 0.0 {
        x += (0.5 - x) * (-p.contrast) * 0.5;
    }
    x += p.highlights * 0.35 * smoothstep(0.5, 1.0, x);
    x += p.shadows * 0.35 * (1.0 - smoothstep(0.0, 0.5, x));
    x += p.whites * 0.2 * smoothstep(0.75, 1.0, x);
    x += p.blacks * 0.2 * (1.0 - smoothstep(0.0, 0.25, x));
    x.clamp(0.0, 1.0)
}

fn linear_to_srgb_clamped(l: f32) -> f32 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }
}

/// Smooth 0..1 ramp from `e0` to `e1`.
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_settings_change_nothing() {
        let p = BasicCorrectionParams::default();
        for v in [0.0, 0.2, 0.5, 0.8, 1.0] {
            assert_eq!(basic_map(v, &p), v);
        }
    }

    #[test]
    fn exposure_brightens_and_blacks_lift_shadows() {
        let up = BasicCorrectionParams { exposure: 1.0, ..Default::default() };
        assert!(basic_map(0.5, &up) > 0.5);
        let lift = BasicCorrectionParams { blacks: 1.0, ..Default::default() };
        assert!(basic_map(0.0, &lift) > 0.0);
        assert_eq!(basic_map(1.0, &lift), 1.0);
    }
}
