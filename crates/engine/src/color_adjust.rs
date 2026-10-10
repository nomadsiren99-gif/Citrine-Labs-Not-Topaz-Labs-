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
        let lab: Vec<[f32; 3]> = img.data().chunks_exact(4).map(|p| to_oklab([p[0], p[1], p[2]])).collect();
        let chroma: Vec<f32> = lab.iter().map(|c| (c[1] * c[1] + c[2] * c[2]).sqrt()).collect();
        let global_mean = chroma.iter().sum::<f32>() / chroma.len() as f32;
        let regions = self.color_regions.clamp(1.0, 10.0);
        let local = box_blur(&chroma, w, h, ((w.max(h) as f32 / (16.0 * regions)).round() as usize).max(1));

        for (i, px) in img.data_mut().chunks_exact_mut(4).enumerate() {
            let [l, a, b] = lab[i];
            let c = chroma[i];
            if c == 0.0 && !lab[i][0].is_finite() {
                continue;
            }
            let ad = (global_mean / local[i].max(1e-6)).clamp(0.5, 2.0);
            let mut c2 = c * (1.0 + adaptive * (ad - 1.0));
            c2 *= saturation;
            c2 *= 1.0 + boost * (1.0 - smoothstep(0.0, 0.25, c));
            let theta = if c > 0.0 { b.atan2(a) + hue } else { hue };
            let rgb = from_oklab([l, c2 * theta.cos(), c2 * theta.sin()]);
            px[0] = rgb[0].max(0.0);
            px[1] = rgb[1].max(0.0);
            px[2] = rgb[2].max(0.0);
        }
        Ok(())
    }
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
    fn rejects_non_finite_parameter() {
        let mut img = colored();
        assert_eq!(
            ColorAdjust { saturation: f32::NAN, ..Default::default() }.apply(&mut img),
            Err(EngineError::NonFiniteParameter("saturation"))
        );
    }
}
