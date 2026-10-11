//! Color v1 (spec Milestone 6). Works on OKLab chroma. Independent proposal, not a
//! reconstruction of Topaz. Mappings are proposed, not verified.

use crate::adaptive_exposure::{box_blur, smoothstep};
use crate::buffer::{EngineError, LinearImage};

/// Parameters for color. `Default` is neutral: the image is returned unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorAdjust {
    /// 0..1. Pulls each pixel's chroma toward the global mean, relative to its local mean.
    pub adaptive_saturation: f32,
    /// 0..2. Global chroma multiplier. 1 = neutral.
    pub saturation: f32,
    /// -1..1. Boosts (positive) or reduces (negative) low-chroma pixels only.
    pub boost: f32,
    /// Hue rotation in degrees, wrapped to 0..360.
    pub hue_deg: f32,
    /// 1..10. Scales how local the adaptive chroma mean is: higher means finer regions.
    pub color_regions: f32,
}

impl Default for ColorAdjust {
    fn default() -> Self {
        Self { adaptive_saturation: 0.0, saturation: 1.0, boost: 0.0, hue_deg: 0.0, color_regions: 1.0 }
    }
}

impl ColorAdjust {
    /// Applies in place. Out-of-range values are clamped; non-finite values are rejected first.
    pub fn apply(&self, img: &mut LinearImage) -> Result<(), EngineError> {
        self.apply_with(img, None)
    }

    /// Like `apply`, but a tile can pass the whole-image chroma mean and size (see `tiled`).
    pub fn apply_with(&self, img: &mut LinearImage, ctx: Option<ColorCtx>) -> Result<(), EngineError> {
        for (name, v) in [
            ("adaptive_saturation", self.adaptive_saturation),
            ("saturation", self.saturation),
            ("boost", self.boost),
            ("hue_deg", self.hue_deg),
            ("color_regions", self.color_regions),
        ] {
            if !v.is_finite() {
                return Err(EngineError::NonFiniteParameter(name));
            }
        }
        let adaptive = self.adaptive_saturation.clamp(0.0, 1.0);
        let saturation = self.saturation.clamp(0.0, 2.0);
        let boost = self.boost.clamp(-1.0, 1.0);
        let hue = self.hue_deg.rem_euclid(360.0).to_radians();
        if adaptive == 0.0 && saturation == 1.0 && boost == 0.0 && hue == 0.0 {
            return Ok(());
        }

        let (w, h) = (img.width(), img.height());
        use rayon::prelude::*;
        let lab: Vec<[f32; 3]> = img.data().par_chunks(4).map(|p| to_oklab([p[0], p[1], p[2]])).collect();
        let chroma: Vec<f32> = lab.par_iter().map(|c| (c[1] * c[1] + c[2] * c[2]).sqrt()).collect();
        let global_mean = match ctx {
            Some(c) => c.global_mean,
            None => chroma.iter().sum::<f32>() / chroma.len() as f32,
        };
        let (fw, fh) = ctx.map(|c| c.full_dims).unwrap_or((w, h));
        let regions = self.color_regions.clamp(1.0, 10.0);
        let local = box_blur(&chroma, w, h, ((fw.max(fh) as f32 / (16.0 * regions)).round() as usize).max(1));

        img.data_mut().par_chunks_mut(4).enumerate().for_each(|(i, px)| {
            let [l, a, b] = lab[i];
            let c = chroma[i];
            if !lab[i][0].is_finite() {
                return;
            }
            let ad = (global_mean / local[i].max(1e-6)).clamp(0.5, 2.0);
            let mut c2 = c * (1.0 + adaptive * (ad - 1.0));
            c2 *= saturation;
            // Boost leaves near-neutral pixels alone (no chroma from nothing), then fades in.
            // shortcut: the absolute cap is a fixed 0.05 OKLab chroma, not tuned per image.
            let floor = smoothstep(0.01, 0.04, c);
            c2 *= 1.0 + boost * floor * (1.0 - smoothstep(0.0, 0.25, c));
            // Adaptive and boost may add at most MAX_ADDED chroma, so a faint cast cannot become a tint.
            c2 = c2.min(c * saturation + MAX_ADDED_CHROMA);
            // Skin keeps most of its colour: chroma increases are scaled down on skin hues.
            if c2 > c {
                c2 = c + (c2 - c) * skin_weight(b.atan2(a).to_degrees());
            }
            // Skin keeps its hue: the rotation is scaled down on skin hues too.
            let base_hue = b.atan2(a);
            let theta = if c > 0.0 { base_hue + hue * skin_weight(base_hue.to_degrees()) } else { hue };
            let rgb = fit_gamut([l, c2 * theta.cos(), c2 * theta.sin()]);
            px[0] = rgb[0].clamp(0.0, 1.0);
            px[1] = rgb[1].clamp(0.0, 1.0);
            px[2] = rgb[2].clamp(0.0, 1.0);
        });
        Ok(())
    }
}

/// Keeps lightness and hue and lowers chroma until the colour is inside the sRGB gamut. A plain
/// per-channel clamp shifts hue when one channel clips, so chroma is reduced instead.
fn fit_gamut(lab: [f32; 3]) -> [f32; 3] {
    let inside = |c: [f32; 3]| c.iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v));
    let rgb = from_oklab(lab);
    if inside(rgb) {
        return rgb;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if inside(from_oklab([lab[0], lab[1] * mid, lab[2] * mid])) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    from_oklab([lab[0], lab[1] * lo, lab[2] * lo])
}

/// Largest chroma (OKLab units) that adaptive saturation and boost may add to one pixel.
const MAX_ADDED_CHROMA: f32 = 0.05;
/// Share of a chroma increase removed at the centre of the skin hue band. Proposed.
const SKIN_PROTECT: f32 = 0.7;

/// Weight on chroma increases by OKLab hue (degrees). Skin sits in roughly 25 to 85 degrees.
/// shortcut: a fixed hue band, not a skin detector; a face-aware mask would be more precise.
fn skin_weight(hue_deg: f32) -> f32 {
    let band = smoothstep(20.0, 35.0, hue_deg) * (1.0 - smoothstep(70.0, 90.0, hue_deg));
    1.0 - SKIN_PROTECT * band
}

/// Whole-image values a tile must reuse so that it matches the full-frame result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorCtx {
    /// Mean OKLab chroma of the whole image, before colour changes.
    pub global_mean: f32,
    /// Full image size, which sets the local averaging radius.
    pub full_dims: (usize, usize),
}

/// Sum and count of OKLab chroma over an image, so a tiled pass can build the whole-image mean.
pub fn chroma_sum(img: &LinearImage) -> (f64, usize) {
    let sum: f64 = img
        .data()
        .chunks_exact(4)
        .map(|p| {
            let c = to_oklab([p[0], p[1], p[2]]);
            (c[1] * c[1] + c[2] * c[2]).sqrt() as f64
        })
        .sum();
    (sum, img.data().len() / 4)
}

fn cbrt(x: f32) -> f32 {
    x.signum() * x.abs().powf(1.0 / 3.0)
}

/// Linear sRGB to OKLab (Ottosson's matrices).
fn to_oklab(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c;
    let l = cbrt(0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_99 * b);
    let m = cbrt(0.211_903_50 * r + 0.680_699_55 * g + 0.107_396_96 * b);
    let s = cbrt(0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_70 * b);
    [
        0.210_454_26 * l + 0.793_617_78 * m - 0.004_072_05 * s,
        1.977_998_50 * l - 2.428_592_20 * m + 0.450_593_71 * s,
        0.025_904_04 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab to linear sRGB. Results may fall outside 0..1; the caller clamps.
fn from_oklab(c: [f32; 3]) -> [f32; 3] {
    let [big_l, a, b] = c;
    let l = (big_l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (big_l - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
    let s = (big_l - 0.089_484_18 * a - 1.291_485_55 * b).powi(3);
    [
        4.076_741_66 * l - 3.307_711_59 * m + 0.230_969_93 * s,
        -1.268_438_00 * l + 2.609_757_40 * m - 0.341_319_40 * s,
        -0.004_196_09 * l - 0.703_418_61 * m + 1.707_614_70 * s,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored() -> LinearImage {
        // 8x4: a row of mid-tone colours, varied enough for chroma statistics to differ.
        let mut rgba = Vec::new();
        for _y in 0..4 {
            for x in 0..8 {
                let k = x as u8 * 20;
                rgba.extend_from_slice(&[200 - k, 60 + k / 2, 90, 255]);
            }
        }
        LinearImage::from_srgb8(8, 4, &rgba).unwrap()
    }

    fn chroma_of(img: &LinearImage, i: usize) -> f32 {
        let p = &img.data()[i * 4..i * 4 + 3];
        let c = to_oklab([p[0], p[1], p[2]]);
        (c[1] * c[1] + c[2] * c[2]).sqrt()
    }

    #[test]
    fn oklab_round_trip_is_accurate() {
        for rgb in [[0.2, 0.5, 0.9], [0.9, 0.1, 0.1], [0.5, 0.5, 0.5]] {
            let back = from_oklab(to_oklab(rgb));
            for c in 0..3 {
                assert!((back[c] - rgb[c]).abs() < 1e-4, "{rgb:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn neutral_defaults_are_bit_identical() {
        let mut img = colored();
        let before = img.clone();
        ColorAdjust::default().apply(&mut img).unwrap();
        assert_eq!(img, before);
    }

    #[test]
    fn zero_saturation_removes_chroma() {
        let mut img = colored();
        ColorAdjust { saturation: 0.0, ..Default::default() }.apply(&mut img).unwrap();
        for i in 0..8 {
            assert!(chroma_of(&img, i) < 1e-3, "pixel {i} still coloured");
        }
    }

    #[test]
    fn saturation_above_one_raises_chroma() {
        let before = colored();
        let mut img = colored();
        ColorAdjust { saturation: 1.5, ..Default::default() }.apply(&mut img).unwrap();
        assert!(chroma_of(&img, 3) > chroma_of(&before, 3) * 1.3);
    }

    #[test]
    fn hue_rotation_preserves_chroma_and_wraps() {
        let before = colored();
        let mut turned = colored();
        ColorAdjust { hue_deg: 120.0, ..Default::default() }.apply(&mut turned).unwrap();
        assert!((chroma_of(&turned, 3) - chroma_of(&before, 3)).abs() < 1e-3);
        let mut full = colored();
        ColorAdjust { hue_deg: 360.0, ..Default::default() }.apply(&mut full).unwrap();
        assert_eq!(full, colored(), "360 degrees is the neutral hue");
    }

    #[test]
    fn boost_favours_low_chroma_pixels() {
        let before = colored();
        let mut img = colored();
        ColorAdjust { boost: 1.0, ..Default::default() }.apply(&mut img).unwrap();
        let chromas: Vec<f32> = (0..8).map(|i| chroma_of(&before, i)).collect();
        let low = (0..8).min_by(|&a, &b| chromas[a].total_cmp(&chromas[b])).unwrap();
        let high = (0..8).max_by(|&a, &b| chromas[a].total_cmp(&chromas[b])).unwrap();
        let gain_low = chroma_of(&img, low) / chromas[low];
        let gain_high = chroma_of(&img, high) / chromas[high];
        assert!(gain_low > gain_high, "low {low} gain {gain_low} vs high {high} gain {gain_high}");
    }

    #[test]
    fn strong_saturation_keeps_hue_inside_gamut() {
        // Regression: per-channel clamping after a saturation boost shifted hue near the gamut edge.
        let mut img = LinearImage::from_srgb8(1, 1, &[250, 120, 40, 255]).unwrap();
        let hue = |img: &LinearImage| {
            let p = &img.data()[0..3];
            let c = to_oklab([p[0], p[1], p[2]]);
            c[2].atan2(c[1]).to_degrees()
        };
        let before = hue(&img);
        ColorAdjust { saturation: 2.0, ..Default::default() }.apply(&mut img).unwrap();
        assert!(img.data()[..3].iter().all(|v| (0.0..=1.0).contains(v)), "left gamut");
        let dh = (hue(&img) - before + 540.0).rem_euclid(360.0) - 180.0;
        assert!(dh.abs() < 1.0, "hue moved {dh:.2} degrees");
    }

    #[test]
    fn rejects_non_finite_parameter() {
        let mut img = colored();
        assert_eq!(
            ColorAdjust { saturation: f32::NAN, ..Default::default() }.apply(&mut img),
            Err(EngineError::NonFiniteParameter("saturation"))
        );
    }
}

#[cfg(test)]
mod gate3_tests {
    //! Gate 3: neutrals stay neutral, and skin does not drift in hue under saturation changes.
    use super::*;

    fn hue_and_chroma(p: &[f32]) -> (f32, f32) {
        let c = to_oklab([p[0], p[1], p[2]]);
        (c[2].atan2(c[1]).to_degrees(), (c[1] * c[1] + c[2] * c[2]).sqrt())
    }

    #[test]
    fn neutral_grey_stays_neutral_under_strong_colour_settings() {
        for v in [40u8, 128, 200, 250] {
            let mut img = LinearImage::from_srgb8(1, 1, &[v, v, v, 255]).unwrap();
            ColorAdjust { saturation: 2.0, adaptive_saturation: 1.0, boost: 1.0, ..Default::default() }
                .apply(&mut img)
                .unwrap();
            let (_, c) = hue_and_chroma(&img.data()[0..3]);
            assert!(c < 2e-3, "grey {v} gained chroma {c:.4}");
        }
    }

    #[test]
    fn skin_hue_does_not_drift_under_saturation() {
        // Typical skin in sRGB; the hue must stay within 3 degrees for moderate boosts.
        for sat in [0.5f32, 1.0, 1.3, 1.6] {
            let mut img = LinearImage::from_srgb8(1, 1, &[224, 172, 140, 255]).unwrap();
            let (h0, _) = hue_and_chroma(&img.data()[0..3]);
            ColorAdjust { saturation: sat, ..Default::default() }.apply(&mut img).unwrap();
            let (h1, _) = hue_and_chroma(&img.data()[0..3]);
            let dh = (h1 - h0 + 540.0).rem_euclid(360.0) - 180.0;
            assert!(dh.abs() < 3.0, "skin hue moved {dh:.2} degrees at saturation {sat}");
        }
    }
}

#[cfg(test)]
mod gate3_cast_tests {
    //! Gate 3: a faint colour cast on a near-neutral surface must not turn into a tint.
    use super::*;

    fn chroma(p: &[f32]) -> f32 {
        let c = to_oklab([p[0], p[1], p[2]]);
        (c[1] * c[1] + c[2] * c[2]).sqrt()
    }

    #[test]
    fn faint_blue_rock_gains_at_most_the_added_chroma_cap() {
        // Blue-grey rock, the kind of surface that turned purple under R18 and R20.
        let mut img = LinearImage::from_srgb8(1, 1, &[120, 118, 130, 255]).unwrap();
        let before = chroma(&img.data()[0..3]);
        ColorAdjust { adaptive_saturation: 1.0, boost: 1.0, saturation: 1.0, ..Default::default() }
            .apply(&mut img)
            .unwrap();
        let after = chroma(&img.data()[0..3]);
        assert!(after <= before + MAX_ADDED_CHROMA + 1e-4, "chroma {before:.4} -> {after:.4}");
    }

    #[test]
    fn near_neutral_pixels_get_no_boost() {
        let mut img = LinearImage::from_srgb8(1, 1, &[128, 127, 129, 255]).unwrap();
        let before = chroma(&img.data()[0..3]);
        ColorAdjust { boost: 1.0, ..Default::default() }.apply(&mut img).unwrap();
        let after = chroma(&img.data()[0..3]);
        assert!(after < before + 1e-3, "near-neutral boosted {before:.5} -> {after:.5}");
    }
}

#[cfg(test)]
mod gate3_skin_tests {
    //! Gate 3: skin keeps its colour under strong saturation; a non-skin colour does not.
    use super::*;

    fn chroma_hue(p: &[f32]) -> (f32, f32) {
        let c = to_oklab([p[0], p[1], p[2]]);
        ((c[1] * c[1] + c[2] * c[2]).sqrt(), c[2].atan2(c[1]).to_degrees())
    }

    /// Chroma gain at saturation 1.8 for one sRGB colour.
    fn chroma_gain(rgb: [u8; 3]) -> f32 {
        let mut img = LinearImage::from_srgb8(1, 1, &[rgb[0], rgb[1], rgb[2], 255]).unwrap();
        let (c0, _) = chroma_hue(&img.data()[0..3]);
        ColorAdjust { saturation: 1.8, ..Default::default() }.apply(&mut img).unwrap();
        let (c1, _) = chroma_hue(&img.data()[0..3]);
        c1 / c0
    }

    #[test]
    fn skin_gains_less_chroma_than_a_non_skin_colour() {
        let skin = chroma_gain([224, 172, 140]);
        let teal = chroma_gain([120, 170, 175]);
        assert!(skin > 1.0, "skin should still get some boost, got {skin:.3}");
        assert!((skin - 1.0) < 0.6 * (teal - 1.0), "skin gain {skin:.3} vs non-skin {teal:.3}");
    }
}
