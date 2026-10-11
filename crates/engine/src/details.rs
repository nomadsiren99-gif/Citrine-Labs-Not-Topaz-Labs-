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
        // Guided base (as in the author's Quartz detail stage): edges stay out of the residue.
        let base = crate::guided::guided_filter_auto(&log_luma, &log_luma, img.width(), img.height(), radius, 2e-2);
        Ok(log_luma.iter().zip(base).map(|(l, b)| l - b).collect())
    }

    /// Scales `img` by the gains applied to `residue`. Pass a residue from before any tone change
    /// to keep detail independent of exposure.
    pub fn apply_residue(&self, img: &mut LinearImage, residue: &[f32]) {
        self.apply_residue_with(img, residue, None);
    }

    /// Like `apply_residue`, with an optional whole-image noise level (see `tiled`).
    pub fn apply_residue_with(&self, img: &mut LinearImage, residue: &[f32], sigma: Option<f32>) {
        let strength = self.strength.clamp(0.0, 2.0);
        let boost = self.boost.clamp(0.0, 2.0);
        let sharpen = self.sharpen.clamp(0.0, 2.0);
        // Higher threshold lowers the cut, so more structures count as strong detail.
        let cut = 0.02 + 0.3 * (1.0 - self.threshold.clamp(0.0, 1.0));

        let sigma = sigma.unwrap_or_else(|| noise_sigma(residue));
        for (px, &d) in img.data_mut().chunks_exact_mut(4).zip(residue) {
            if !luma(px).is_finite() {
                continue;
            }
            let admit = smoothstep(0.5 * cut, cut, d.abs());
            let gain = boost + (strength - boost) * admit;
            // Amplifying gain (boost, sharpen) must not act on noise-level residue. Smoothing does.
            let mut k = (gain - 1.0) + 0.5 * sharpen;
            if k > 0.0 && sigma > 1e-4 {
                k *= smoothstep(2.0 * sigma, 4.0 * sigma, d.abs());
            }
            let delta = k * d;
            let g = delta.clamp(-MAX_DETAIL_EV, MAX_DETAIL_EV).exp2();
            px[0] *= g;
            px[1] *= g;
            px[2] *= g;
        }
    }
}

/// Histogram bins for the noise estimate. The median is read from bins over |residue| in 0..2
/// log2, so a tile can add its counts to the whole-image histogram and get the same result as a
/// full-frame run. Bin width is 2/4096 log2, far below anything visible.
pub(crate) const SIGMA_BINS: usize = 4096;
pub(crate) const SIGMA_MAX: f32 = 2.0;

/// Histogram of |residue|, clamped to the bin range.
pub(crate) fn abs_histogram(residue: &[f32]) -> Vec<u64> {
    let mut hist = vec![0u64; SIGMA_BINS];
    for d in residue {
        let bin = ((d.abs() / SIGMA_MAX) * SIGMA_BINS as f32) as usize;
        hist[bin.min(SIGMA_BINS - 1)] += 1;
    }
    hist
}

/// Robust noise level from a |residue| histogram: 1.4826 times the median.
pub(crate) fn sigma_from_histogram(hist: &[u64]) -> f32 {
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let mut acc = 0u64;
    for (bin, &n) in hist.iter().enumerate() {
        acc += n;
        if acc * 2 >= total {
            return 1.4826 * (bin as f32 + 0.5) * SIGMA_MAX / SIGMA_BINS as f32;
        }
    }
    0.0
}

/// Robust noise level of the residue (see `sigma_from_histogram`).
pub(crate) fn noise_sigma(residue: &[f32]) -> f32 {
    sigma_from_histogram(&abs_histogram(residue))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat grey with deterministic pseudo-random noise of about 3 code values (LCG, no deps).
    fn flat_noise() -> LinearImage {
        let mut s: u32 = 12345;
        let mut rgba = Vec::with_capacity(64 * 64 * 4);
        for _ in 0..64 * 64 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u = (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
            let v = (128.0 + 6.0 * u).round() as u8;
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
        LinearImage::from_srgb8(64, 64, &rgba).unwrap()
    }

    fn std_of_luma(img: &LinearImage) -> f32 {
        let l: Vec<f32> = img.data().chunks_exact(4).map(luma).collect();
        let m = l.iter().sum::<f32>() / l.len() as f32;
        (l.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / l.len() as f32).sqrt()
    }

    #[test]
    fn boost_and_sharpen_do_not_amplify_flat_noise() {
        // Regression: boost 1.3 plus sharpen 1.0 used to raise flat-patch noise by about 1.65x.
        let before = flat_noise();
        let mut img = before.clone();
        Details { strength: 1.07, boost: 1.30, threshold: 0.08, sharpen: 1.0, radius: 4.0, ..Default::default() }
            .apply(&mut img)
            .unwrap();
        let ratio = std_of_luma(&img) / std_of_luma(&before);
        assert!(ratio < 1.1, "noise amplified by {ratio:.2}x");
    }

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

#[cfg(test)]
mod gate2_tests {
    //! Gate 2: sharpening must not ring. Overshoot beyond the plateau is measured against the step.
    use super::*;

    /// 64x1 step, sRGB 60 to 180 at the middle.
    fn edge() -> LinearImage {
        let mut rgba = Vec::new();
        for x in 0..64 {
            let v = if x < 32 { 60u8 } else { 180u8 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
        LinearImage::from_srgb8(64, 1, &rgba).unwrap()
    }

    /// Largest excursion past the far plateau, as a fraction of the step in luma.
    fn overshoot(img: &LinearImage, before: &LinearImage) -> f32 {
        let l = |i: &LinearImage, x: usize| luma(&i.data()[x * 4..]);
        let (lo, hi) = (l(before, 4), l(before, 60));
        let step = (hi - lo).abs();
        let mut worst = 0.0f32;
        for x in 0..64 {
            let v = l(img, x);
            let past = if x < 32 { (lo - v).max(0.0) } else { (v - hi).max(0.0) };
            worst = worst.max(past);
        }
        worst / step
    }

    #[test]
    fn sharpening_overshoot_is_bounded() {
        let before = edge();
        for (name, d) in [
            ("default", Details::default()),
            ("medium", Details { strength: 1.07, boost: 1.30, threshold: 0.08, radius: 2.0, sharpen: 1.0, process_independently: false }),
            ("strong", Details { strength: 1.5, boost: 1.0, threshold: 0.5, radius: 2.0, sharpen: 2.0, process_independently: false }),
        ] {
            let mut img = before.clone();
            d.apply(&mut img).unwrap();
            let o = overshoot(&img, &before);
            eprintln!("gate2 {name}: overshoot {:.2}% of the step", o * 100.0);
            assert!(o < 0.02, "{name}: ringing {:.1}% of the step", o * 100.0);
        }
    }
}
