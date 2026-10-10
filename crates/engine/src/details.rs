//! Details v1 (spec Milestone 5). Independent proposal: detail is the difference between
//! log-luminance and a box-blur base. Gains scale that difference, and a halo guard clamps the
//! change. Mappings are proposed, not verified against Topaz.

use crate::adaptive_exposure::{luma, smoothstep};
use crate::buffer::{EngineError, LinearImage};

/// Largest change to log-luminance from detail work, in stops. Halo guard. Proposed.
pub const MAX_DETAIL_EV: f32 = 1.0;
const LUMA_EPS: f32 = 1e-6;

/// Parameters for Details. `Default` is neutral: the image is returned unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Details {
    /// Gain on admitted (strong) detail. 1 = neutral, below 1 softens, above 1 enhances.
    pub strength: f32,
    /// Gain on weak detail that the threshold does not admit. 1 = neutral.
    pub boost: f32,
    /// 0..1. Higher admits more structures into the strength gain.
    pub threshold: f32,
    /// Base blur radius in pixels.
    pub radius: f32,
    /// 0..2. Extra gain on the detail itself, limited by the halo guard.
    pub sharpen: f32,
    /// Take detail from the input before tone changes, so exposure does not move it.
    pub process_independently: bool,
}

impl Default for Details {
    fn default() -> Self {
        Self { strength: 1.0, boost: 1.0, threshold: 0.5, radius: 1.5, sharpen: 0.0, process_independently: false }
    }
}

impl Details {
    /// Applies the adjustment in place. Out-of-range values are clamped; non-finite values are
    /// rejected before any pixel changes.
    pub fn apply(&self, img: &mut LinearImage) -> Result<(), EngineError> {
        let residue = self.residue(img)?;
        self.apply_residue(img, &residue);
        Ok(())
    }

    /// Log2-luminance minus its blurred base, one value per pixel. Taken from `img` as it is now.
    pub fn residue(&self, img: &LinearImage) -> Result<Vec<f32>, EngineError> {
        for (name, v) in [
            ("strength", self.strength),
            ("boost", self.boost),
            ("threshold", self.threshold),
            ("radius", self.radius),
            ("sharpen", self.sharpen),
        ] {
            if !v.is_finite() {
                return Err(EngineError::NonFiniteParameter(name));
            }
        }
        let radius = self.radius.clamp(1.0, 16.0).round() as usize;
        let log_luma: Vec<f32> = img
            .data()
            .chunks_exact(4)
            .map(|p| luma(p).max(LUMA_EPS).log2())
            .collect();
        // Guided base (as in the author's Limestone detail stage): edges stay out of the residue.
        let base = crate::guided::guided_filter(&log_luma, &log_luma, img.width(), img.height(), radius, 2e-2);
        Ok(log_luma.iter().zip(base).map(|(l, b)| l - b).collect())
    }

    /// Scales `img` by the gains applied to `residue`. Pass a residue from before any tone change
    /// to keep detail independent of exposure.
    pub fn apply_residue(&self, img: &mut LinearImage, residue: &[f32]) {
        let strength = self.strength.clamp(0.0, 2.0);
        let boost = self.boost.clamp(0.0, 2.0);
        let sharpen = self.sharpen.clamp(0.0, 2.0);
        // Higher threshold lowers the cut, so more structures count as strong detail.
        let cut = 0.02 + 0.3 * (1.0 - self.threshold.clamp(0.0, 1.0));

        for (px, &d) in img.data_mut().chunks_exact_mut(4).zip(residue) {
            if !luma(px).is_finite() {
                continue;
            }
            let admit = smoothstep(0.5 * cut, cut, d.abs());
            let gain = boost + (strength - boost) * admit;
            let delta = ((gain - 1.0) + 0.5 * sharpen) * d;
            let g = delta.clamp(-MAX_DETAIL_EV, MAX_DETAIL_EV).exp2();
            px[0] *= g;
            px[1] *= g;
            px[2] *= g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 40x4 image: columns 0..20 are sRGB 100, columns 20..40 are sRGB 200.
    fn step_image() -> LinearImage {
        let mut rgba = Vec::with_capacity(40 * 4 * 4);
        for _y in 0..4 {
            for x in 0..40 {
                let v = if x < 20 { 100 } else { 200 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        LinearImage::from_srgb8(40, 4, &rgba).unwrap()
    }

    /// Fine texture: alternating columns 100 and 120. Small detail, no single edge.
    fn texture_image() -> LinearImage {
        let mut rgba = Vec::with_capacity(40 * 4 * 4);
        for _y in 0..4 {
            for x in 0..40 {
                let v = if x % 2 == 0 { 100 } else { 120 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        LinearImage::from_srgb8(40, 4, &rgba).unwrap()
    }

    fn spread_of(img: &LinearImage) -> f32 {
        let l: Vec<f32> = (0..40).map(|x| lum_at(img, x)).collect();
        l.iter().cloned().fold(f32::MIN, f32::max) - l.iter().cloned().fold(f32::MAX, f32::min)
    }

    fn lum_at(img: &LinearImage, x: usize) -> f32 {
        let i = (1 * img.width() + x) * 4;
        luma(&img.data()[i..i + 4])
    }

    #[test]
    fn neutral_defaults_are_bit_identical() {
        let mut img = step_image();
        let before = img.clone();
        Details::default().apply(&mut img).unwrap();
        assert_eq!(img, before);
    }

    #[test]
    fn boost_below_one_softens_fine_texture() {
        // Texture is weak, so the boost gain applies. Boost 0 flattens it; boost 1 leaves it.
        let mut soft = texture_image();
        let mut neutral = texture_image();
        Details { boost: 0.0, ..Default::default() }.apply(&mut soft).unwrap();
        Details::default().apply(&mut neutral).unwrap();
        assert!(spread_of(&soft) < spread_of(&neutral));
    }

    #[test]
    fn halo_guard_bounds_the_change_to_one_stop() {
        let before = step_image();
        let mut img = before.clone();
        Details { strength: 2.0, sharpen: 2.0, ..Default::default() }.apply(&mut img).unwrap();
        let limit = (2.0f32).powf(MAX_DETAIL_EV) * (1.0 + 1e-4);
        for x in 0..40 {
            let ratio = lum_at(&img, x) / lum_at(&before, x);
            assert!(ratio <= limit && ratio >= 1.0 / limit, "x={x} ratio={ratio}");
        }
    }

    /// Same layout as `step_image`, but a small step (sRGB 100 to 110) that sits below the
    /// default threshold, so the boost gain applies.
    fn weak_step_image() -> LinearImage {
        let mut rgba = Vec::with_capacity(40 * 4 * 4);
        for _y in 0..4 {
            for x in 0..40 {
                let v = if x < 20 { 100 } else { 110 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        LinearImage::from_srgb8(40, 4, &rgba).unwrap()
    }

    #[test]
    fn boost_above_one_enhances_weak_detail() {
        let mut low = weak_step_image();
        let mut high = weak_step_image();
        Details { strength: 1.0, boost: 0.0, ..Default::default() }.apply(&mut low).unwrap();
        Details { strength: 1.0, boost: 2.0, ..Default::default() }.apply(&mut high).unwrap();
        assert!(lum_at(&high, 20) > lum_at(&low, 20));
    }

    #[test]
    fn higher_threshold_admits_more_texture_into_the_strength_gain() {
        // Strength 0 flattens admitted texture only. Threshold 1 admits this texture, so it
        // is flattened; threshold 0 leaves it to the neutral boost.
        let mut none = texture_image();
        let mut all = texture_image();
        Details { strength: 0.0, threshold: 0.0, ..Default::default() }.apply(&mut none).unwrap();
        Details { strength: 0.0, threshold: 1.0, ..Default::default() }.apply(&mut all).unwrap();
        assert!(spread_of(&all) < spread_of(&none));
    }

    #[test]
    fn rejects_non_finite_parameter() {
        let mut img = step_image();
        assert_eq!(
            Details { radius: f32::NAN, ..Default::default() }.apply(&mut img),
            Err(EngineError::NonFiniteParameter("radius"))
        );
    }
}
