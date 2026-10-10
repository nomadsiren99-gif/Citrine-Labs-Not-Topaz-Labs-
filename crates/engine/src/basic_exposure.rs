//! Basic exposure (first stage of the look). Contrast pivots around mid-grey in log space.
//! Black and white boosts lift shadows and highlights separately. Independent proposal.
//! Brightness is the Look's global EV, set in `pipeline`, not here. Mappings proposed.

use crate::adaptive_exposure::{luma, smoothstep};
use crate::buffer::{EngineError, LinearImage};

const MID_LOG: f32 = -2.47; // log2 of linear 0.18
const LUMA_EPS: f32 = 1e-6;
/// Largest lift from a black or white boost, in stops. Proposed.
const MAX_BOOST_EV: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BasicExposure {
    /// -1..1. Scales the log-space distance from mid-grey by (1 + 0.5 c).
    pub contrast: f32,
    /// -1..1. Lifts (positive) or crushes (negative) shadows.
    pub boost_blacks: f32,
    /// -1..1. Lifts (positive) or pulls down (negative) highlights.
    pub boost_whites: f32,
}

impl BasicExposure {
    pub fn apply(&self, img: &mut LinearImage) -> Result<(), EngineError> {
        for (name, v) in [("contrast", self.contrast), ("boost_blacks", self.boost_blacks), ("boost_whites", self.boost_whites)] {
            if !v.is_finite() {
                return Err(EngineError::NonFiniteParameter(name));
            }
        }
        let c = self.contrast.clamp(-1.0, 1.0);
        let kb = self.boost_blacks.clamp(-1.0, 1.0);
        let kw = self.boost_whites.clamp(-1.0, 1.0);
        if c == 0.0 && kb == 0.0 && kw == 0.0 {
            return Ok(());
        }
        let k = 1.0 + 0.5 * c;
        for px in img.data_mut().chunks_exact_mut(4) {
            let y = luma(px);
            if !y.is_finite() {
                continue;
            }
            let ly = y.max(LUMA_EPS).log2();
            let shadows = 1.0 - smoothstep(0.0, 0.25, y);
            let highlights = smoothstep(0.5, 1.0, y);
            let target = MID_LOG + k * (ly - MID_LOG);
            let lift = kb * MAX_BOOST_EV * shadows + kw * MAX_BOOST_EV * highlights;
            let g = (target - ly + lift).exp2();
            px[0] *= g;
            px[1] *= g;
            px[2] *= g;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LinearImage {
        let mut rgba = Vec::new();
        for v in [30u8, 90, 140, 200, 240] {
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
        LinearImage::from_srgb8(5, 1, &rgba).unwrap()
    }

    #[test]
    fn neutral_is_bit_identical() {
        let mut img = sample();
        let before = img.clone();
        BasicExposure::default().apply(&mut img).unwrap();
        assert_eq!(img, before);
    }

    #[test]
    fn more_contrast_spreads_the_tones() {
        let before = sample();
        let mut img = sample();
        BasicExposure { contrast: 1.0, ..Default::default() }.apply(&mut img).unwrap();
        // Darkest pixel gets darker, brightest gets brighter.
        assert!(img.data()[0] < before.data()[0]);
        assert!(img.data()[16] > before.data()[16]);
    }

    #[test]
    fn black_boost_lifts_shadows_only() {
        let before = sample();
        let mut img = sample();
        BasicExposure { boost_blacks: 1.0, ..Default::default() }.apply(&mut img).unwrap();
        assert!(img.data()[0] > before.data()[0]);
        assert!((img.data()[16] - before.data()[16]).abs() < 1e-5);
    }

    #[test]
    fn rejects_non_finite() {
        let mut img = sample();
        assert_eq!(
            BasicExposure { contrast: f32::NAN, ..Default::default() }.apply(&mut img),
            Err(EngineError::NonFiniteParameter("contrast"))
        );
    }
}
