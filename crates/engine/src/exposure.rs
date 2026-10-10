use crate::buffer::{EngineError, LinearImage};

/// Exposure is clamped to +/-10 EV. Out-of-range values are clamped, not rejected.
pub const MAX_EV: f32 = 10.0;

/// Global exposure: multiplies linear RGB by 2^ev. Alpha is untouched; ev = 0 is identity.
/// Non-finite ev is rejected because it cannot define a gain.
pub fn apply_exposure(img: &mut LinearImage, ev: f32) -> Result<(), EngineError> {
    if !ev.is_finite() {
        return Err(EngineError::NonFiniteParameter("exposure ev"));
    }
    let gain = 2f32.powf(ev.clamp(-MAX_EV, MAX_EV));
    for px in img.data_mut().chunks_exact_mut(4) {
        px[0] *= gain;
        px[1] *= gain;
        px[2] *= gain;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two pixels: opaque mid-grey and the same grey at alpha 128.
    fn sample() -> LinearImage {
        LinearImage::from_srgb8(2, 1, &[100, 100, 100, 255, 100, 100, 100, 128]).unwrap()
    }

    #[test]
    fn zero_ev_is_identity() {
        let mut img = sample();
        let before = img.clone();
        apply_exposure(&mut img, 0.0).unwrap();
        assert_eq!(img, before);
    }

    #[test]
    fn plus_one_stop_doubles_linear_rgb_and_keeps_alpha() {
        let mut img = sample();
        let before = img.clone();
        apply_exposure(&mut img, 1.0).unwrap();
        for (a, b) in before.data().chunks_exact(4).zip(img.data().chunks_exact(4)) {
            for c in 0..3 {
                assert!((b[c] - 2.0 * a[c]).abs() < 1e-6);
            }
            assert_eq!(a[3], b[3]);
        }
    }

    #[test]
    fn negative_stop_halves_linear_rgb() {
        let mut img = sample();
        let before = img.clone();
        apply_exposure(&mut img, -1.0).unwrap();
        assert!((img.data()[0] - 0.5 * before.data()[0]).abs() < 1e-6);
    }

    #[test]
    fn plus_one_stop_matches_independent_f64_formula() {
        // Expected value is computed from the sRGB equations directly, not through LinearImage.
        fn to_lin(v: f64) -> f64 {
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        }
        fn to_srgb(v: f64) -> f64 {
            if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
        }
        let mut img = sample();
        apply_exposure(&mut img, 1.0).unwrap();
        let out = img.to_srgb8();
        let expected = (to_srgb(to_lin(100.0 / 255.0) * 2.0) * 255.0).round() as i32;
        for c in 0..3 {
            assert!((out[c] as i32 - expected).abs() <= 1, "channel {c}: {} vs {expected}", out[c]);
        }
        assert_eq!(out[3], 255);
        assert_eq!(out[7], 128);
    }

    #[test]
    fn rejects_non_finite_ev_and_clamps_large_ev() {
        let mut img = sample();
        assert_eq!(
            apply_exposure(&mut img, f32::NAN),
            Err(EngineError::NonFiniteParameter("exposure ev"))
        );
        let mut huge = sample();
        let mut capped = sample();
        apply_exposure(&mut huge, 1000.0).unwrap();
        apply_exposure(&mut capped, MAX_EV).unwrap();
        assert_eq!(huge, capped);
    }
}
