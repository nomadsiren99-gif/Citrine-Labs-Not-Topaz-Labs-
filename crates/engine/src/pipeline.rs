//! Runs adaptive exposure and details in order. With `process_independently`, detail is taken
//! from the input before tone mapping, so exposure does not change it.

use crate::adaptive_exposure::AdaptiveExposure;
use crate::basic_exposure::BasicExposure;
use crate::buffer::{EngineError, LinearImage};
use crate::color_adjust::ColorAdjust;
use crate::curve::Curve;
use crate::details::Details;
use crate::exposure::apply_exposure;
use crate::finishing::Finishing;
use crate::local::{self, Stroke};
use crate::noise::Denoise;

pub fn apply(img: &mut LinearImage, adaptive: &AdaptiveExposure, details: &Details) -> Result<(), EngineError> {
    if details.process_independently {
        let residue = details.residue(img)?;
        adaptive.apply(img)?;
        details.apply_residue(img, &residue);
        Ok(())
    } else {
        adaptive.apply(img)?;
        details.apply(img)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LinearImage {
        let mut rgba = Vec::new();
        for _y in 0..4 {
            for x in 0..40 {
                let v = if x < 20 { 60 } else { 180 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        LinearImage::from_srgb8(40, 4, &rgba).unwrap()
    }

    #[test]
    fn independent_mode_matches_dependent_when_details_are_neutral() {
        let adaptive = AdaptiveExposure { strength: 0.5, regions: 2, brightness_ev: 0.5, ..Default::default() };
        let mut dep = sample();
        let mut ind = sample();
        apply(&mut dep, &adaptive, &Details::default()).unwrap();
        apply(&mut ind, &adaptive, &Details { process_independently: true, ..Default::default() }).unwrap();
        assert_eq!(dep, ind);
    }

    #[test]
    fn pure_exposure_leaves_independent_detail_unchanged() {
        // Brightness is a pure global gain. Independent detail is measured before it, so the
        // detail residue stays the same, and a neutral detail stage is an exact no-op on top.
        let adaptive = AdaptiveExposure { brightness_ev: 1.0, ..Default::default() };
        let mut img = sample();
        let before = img.clone();
        apply(&mut img, &adaptive, &Details { process_independently: true, ..Default::default() }).unwrap();
        for (a, b) in before.data().chunks_exact(4).zip(img.data().chunks_exact(4)) {
            assert!((b[0] / a[0] - 2.0).abs() < 1e-5);
        }
    }
}

/// Which stage set runs. `Ours` is the default engine. `Limestone` is the author's ported set.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Style {
    #[default]
    Ours,
    Limestone,
}

/// Every control in one place, neutral by default. Order follows the spec: exposure, adaptive
/// exposure, details, colour, noise, curve, local strokes, finishing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Look {
    pub style: Style,
    pub ev: f32,
    pub basic: BasicExposure,
    pub adaptive: AdaptiveExposure,
    pub details: Details,
    pub color: ColorAdjust,
    pub denoise: Denoise,
    pub curve: Curve,
    pub strokes: Vec<Stroke>,
    pub finishing: Finishing,
}

/// Applies a full look. Stroke and transparency references use the image as it was before the
/// look, so Brush Out and Transparency return toward the input.
pub fn apply_look(img: &mut LinearImage, look: &Look) -> Result<(), EngineError> {
    let original = img.clone();
    if look.style == Style::Limestone {
        apply_exposure(img, look.ev)?;
        return crate::limestone::apply(img, &original, look);
    }
    apply_exposure(img, look.ev)?;
    look.basic.apply(img)?;
    apply(img, &look.adaptive, &look.details)?;
    look.color.apply(img)?;
    look.denoise.apply(img)?;
    look.curve.apply(img)?;
    local::apply(img, &original, &look.strokes)?;
    look.finishing.apply(img, &original)
}

#[cfg(test)]
mod look_tests {
    use super::*;

    #[test]
    fn default_look_is_bit_identical() {
        let mut rgba = Vec::new();
        for i in 0..64u8 {
            rgba.extend_from_slice(&[i * 3, 255 - i * 3, 90, 255]);
        }
        let original = LinearImage::from_srgb8(8, 8, &rgba).unwrap();
        let mut img = original.clone();
        apply_look(&mut img, &Look::default()).unwrap();
        assert_eq!(img, original);
    }
}
