use crate::color::{hsv_to_rgb, rgb_to_hsv};
use crate::filter::box_filter;
use crate::image_buffer::RgbImageF32;
use crate::params::ColorParams;
use crate::pipeline::RenderContext;
use rayon::prelude::*;

/// Averaging radius of the regional saturation, in pixels. Also used for the tile margin.
/// Reference saturation for the regional term (a neutral mid value).
const REGION_REF_SAT: f32 = 0.5;

pub fn color_radius_px(params: &ColorParams, ctx: &RenderContext) -> usize {
    let max_dim = ctx.full_width.max(ctx.full_height) as f32;
    ((max_dim / (16.0 * params.color_regions.clamp(1, 30) as f32)).round() as usize).max(1)
}

pub fn apply_color_adjust(img: &mut RgbImageF32, params: &ColorParams, ctx: &RenderContext) {
    if !params.enabled {
        return;
    }

    if params.adaptive_saturation == 0.0
        && params.saturation == 0.0
        && params.saturation_boost == 1.0
        && params.hue == 0.0
    {
        return;
    }

    let adapt_sat = params.adaptive_saturation * 0.7;
    let sat_boost = params.saturation_boost;
    let global_sat = params.saturation;
    let hue_shift = params.hue / 360.0;

    // Regional adaptive saturation: a pixel's share of the boost is scaled by how its
    // saturation compares with the saturation around it. Muted areas inside vivid ones
    // get lifted more, vivid areas inside muted ones less. The reference is a fixed mid
    // saturation, not the image mean, so a tile gets the same result as the full frame.
    let (w, h) = (img.width, img.height);
    let sat: Vec<f32> = (0..w * h).into_par_iter().map(|i| rgb_to_hsv(img.r[i], img.g[i], img.b[i]).1).collect();
    let local = box_filter(&sat, w, h, color_radius_px(params, ctx));

    img.r
        .par_iter_mut()
        .zip(img.g.par_iter_mut())
        .zip(img.b.par_iter_mut())
        .zip(local.par_iter())
        .for_each(|(((r, g), b), local_sat)| {
            let (mut h, mut s, v) = rgb_to_hsv(*r, *g, *b);
            let ratio = (REGION_REF_SAT / local_sat.max(1e-4)).clamp(0.5, 2.0);

            // 1. Hue shift
            if hue_shift != 0.0 {
                h = (h + hue_shift) % 1.0;
                if h < 0.0 {
                    h += 1.0;
                }
            }

            // 2. Adaptive saturation (vibrance): boosts muted colours most, but scales
            //    with existing chroma so neutral greys don't pick up random hue noise.
            if adapt_sat > 0.0 {
                s += adapt_sat * sat_boost * ratio * 2.0 * s * (1.0 - s);
            } else if adapt_sat < 0.0 {
                // Desaturate vibrant colours
                s += s * adapt_sat * 0.5 * ratio;
            }

            // 3. Global saturation (multiplicative, so greys stay grey)
            s *= 1.0 + global_sat;

            s = s.clamp(0.0, 1.0);

            let (nr, ng, nb) = hsv_to_rgb(h, s, v);
            *r = nr.clamp(0.0, 1.0);
            *g = ng.clamp(0.0, 1.0);
            *b = nb.clamp(0.0, 1.0);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ColorParams {
        ColorParams { adaptive_saturation: 1.0, saturation_boost: 1.0, ..ColorParams::default() }
    }

    /// Left half vivid (s = 0.8), right half muted (s = 0.2), same hue and value.
    fn split_image() -> RgbImageF32 {
        let (w, h) = (64, 64);
        let mut img = RgbImageF32::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let s = if x < w / 2 { 0.8 } else { 0.2 };
                let (r, g, b) = hsv_to_rgb(0.0, s, 0.8);
                let i = y * w + x;
                img.r[i] = r;
                img.g[i] = g;
                img.b[i] = b;
            }
        }
        img
    }

    fn sat_at(img: &RgbImageF32, x: usize, y: usize) -> f32 {
        let i = y * img.width + x;
        rgb_to_hsv(img.r[i], img.g[i], img.b[i]).1
    }

    #[test]
    fn muted_area_gains_more_than_vivid_area() {
        let mut img = split_image();
        let ctx = RenderContext::whole(&img);
        apply_color_adjust(&mut img, &params(), &ctx);
        let muted_gain = sat_at(&img, 56, 32) / 0.2;
        let vivid_gain = sat_at(&img, 8, 32) / 0.8;
        assert!(muted_gain > vivid_gain, "muted {muted_gain} should gain more than vivid {vivid_gain}");
    }

    #[test]
    fn uniform_image_is_unchanged_by_regions() {
        let mut flat = RgbImageF32::new(16, 16);
        flat.r.fill(0.6);
        flat.g.fill(0.4);
        flat.b.fill(0.3);
        let flat0 = flat.clone();
        let mut a = flat.clone();
        let mut b = flat;
        apply_color_adjust(&mut a, &ColorParams { color_regions: 1, ..params() }, &RenderContext::whole(&flat0));
        apply_color_adjust(&mut b, &ColorParams { color_regions: 30, ..params() }, &RenderContext::whole(&flat0));
        assert_eq!(a.r, b.r);
        assert_eq!(a.g, b.g);
        assert_eq!(a.b, b.b);
    }
}
