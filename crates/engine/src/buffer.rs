use std::fmt;

use rayon::prelude::*;

use crate::color::{linear_to_srgb, srgb_to_linear};

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    InvalidDimensions { width: usize, height: usize },
    LengthMismatch { expected: usize, actual: usize },
    NonFiniteParameter(&'static str),
    InvalidParameter(&'static str),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions { width, height } => {
                write!(f, "invalid image dimensions {width}x{height}")
            }
            Self::LengthMismatch { expected, actual } => {
                write!(f, "buffer length {actual}, expected {expected}")
            }
            Self::NonFiniteParameter(name) => write!(f, "parameter `{name}` is not finite"),
            Self::InvalidParameter(what) => write!(f, "invalid parameter: {what}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Interleaved RGBA, f32, linear light with sRGB primaries.
/// Alpha is straight (not premultiplied) and is never transfer-encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearImage {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

fn sample_count(width: usize, height: usize) -> Result<usize, EngineError> {
    width
        .checked_mul(height)
        .and_then(|p| p.checked_mul(4))
        .filter(|_| width > 0 && height > 0)
        .ok_or(EngineError::InvalidDimensions { width, height })
}

impl LinearImage {
    pub fn new(width: usize, height: usize, data: Vec<f32>) -> Result<Self, EngineError> {
        let expected = sample_count(width, height)?;
        if data.len() != expected {
            return Err(EngineError::LengthMismatch {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self { width, height, data })
    }

    /// Decodes 8-bit sRGB RGBA. Alpha is scaled to 0..1 without a transfer curve.
    pub fn from_srgb8(width: usize, height: usize, rgba: &[u8]) -> Result<Self, EngineError> {
        let expected = sample_count(width, height)?;
        if rgba.len() != expected {
            return Err(EngineError::LengthMismatch {
                expected,
                actual: rgba.len(),
            });
        }
        // Exact: an 8-bit input has only 256 values, so the table is the same as the formula.
        let lut: Vec<f32> = (0..256).map(|v| srgb_to_linear(v as f32 / 255.0)).collect();
        let mut data = vec![0.0f32; rgba.len()];
        data.par_chunks_mut(4).zip(rgba.par_chunks(4)).for_each(|(out, p)| {
            out[0] = lut[p[0] as usize];
            out[1] = lut[p[1] as usize];
            out[2] = lut[p[2] as usize];
            out[3] = p[3] as f32 / 255.0;
        });
        Ok(Self { width, height, data })
    }

    /// Decodes 16-bit sRGB RGBA. Alpha is scaled to 0..1 without a transfer curve.
    pub fn from_srgb16(width: usize, height: usize, rgba: &[u16]) -> Result<Self, EngineError> {
        let expected = sample_count(width, height)?;
        if rgba.len() != expected {
            return Err(EngineError::LengthMismatch { expected, actual: rgba.len() });
        }
        let mut data = vec![0.0f32; rgba.len()];
        data.par_chunks_mut(4).zip(rgba.par_chunks(4)).for_each(|(out, p)| {
            out[0] = srgb_to_linear(p[0] as f32 / 65535.0);
            out[1] = srgb_to_linear(p[1] as f32 / 65535.0);
            out[2] = srgb_to_linear(p[2] as f32 / 65535.0);
            out[3] = p[3] as f32 / 65535.0;
        });
        Ok(Self { width, height, data })
    }

    /// Encodes to 16-bit sRGB RGBA. Non-finite values become 0; out-of-range values are clamped.
    pub fn to_srgb16(&self) -> Vec<u16> {
        let mut out = vec![0u16; self.data.len()];
        out.par_chunks_mut(4).zip(self.data.par_chunks(4)).for_each(|(o, p)| {
            o[0] = to_unit_u16(linear_to_srgb(clean(p[0])));
            o[1] = to_unit_u16(linear_to_srgb(clean(p[1])));
            o[2] = to_unit_u16(linear_to_srgb(clean(p[2])));
            o[3] = to_unit_u16(clean(p[3]));
        });
        out
    }

    /// Encodes to 8-bit sRGB RGBA. Non-finite values become 0; out-of-range values are clamped.
    pub fn to_srgb8(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.data.len()];
        out.par_chunks_mut(4).zip(self.data.par_chunks(4)).for_each(|(o, p)| {
            o[0] = encode_channel(p[0]);
            o[1] = encode_channel(p[1]);
            o[2] = encode_channel(p[2]);
            o[3] = to_unit_u8(p[3]);
        });
        out
    }

    /// Box-average downscale by an integer factor. Averages in linear light, so the level matches
    /// what a full-size render looks like at that scale. Partial blocks at the edge are averaged
    /// over the pixels that exist.
    pub fn downscale(&self, factor: usize) -> Self {
        let factor = factor.max(1);
        let (nw, nh) = (self.width.div_ceil(factor), self.height.div_ceil(factor));
        let mut data = vec![0.0f32; nw * nh * 4];
        for ny in 0..nh {
            for nx in 0..nw {
                let mut sum = [0.0f32; 4];
                let mut n = 0.0f32;
                for y in ny * factor..((ny + 1) * factor).min(self.height) {
                    for x in nx * factor..((nx + 1) * factor).min(self.width) {
                        let i = (y * self.width + x) * 4;
                        for c in 0..4 {
                            sum[c] += self.data[i + c];
                        }
                        n += 1.0;
                    }
                }
                let o = (ny * nw + nx) * 4;
                for c in 0..4 {
                    data[o + c] = sum[c] / n;
                }
            }
        }
        Self { width: nw, height: nh, data }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn data(&self) -> &[f32] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }
}

fn encode_channel(linear: f32) -> u8 {
    to_unit_u8(linear_to_srgb(clean(linear)))
}

fn clean(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

fn to_unit_u16(v: f32) -> u16 {
    (clean(v) * 65535.0).round() as u16
}

fn to_unit_u8(v: f32) -> u8 {
    (clean(v) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_8bit_value_round_trips_exactly() {
        for v in 0..=255u8 {
            let img = LinearImage::from_srgb8(1, 1, &[v, v, v, v]).unwrap();
            assert_eq!(img.to_srgb8(), vec![v, v, v, v], "value {v}");
        }
    }

    #[test]
    fn every_16bit_value_round_trips_exactly() {
        // Gate 0: a neutral 16-bit pipeline must return every code value it was given.
        let all: Vec<u16> = (0..=65535u16).flat_map(|v| [v, v, v, v]).collect();
        let img = LinearImage::from_srgb16(65536, 1, &all).unwrap();
        let back = img.to_srgb16();
        let bad = back.iter().zip(&all).filter(|(a, b)| a != b).count();
        assert_eq!(bad, 0, "{bad} channel values changed");
    }

    #[test]
    fn alpha_is_kept_exactly_in_16_bit() {
        let img = LinearImage::from_srgb16(1, 1, &[1000, 2000, 3000, 12345]).unwrap();
        assert_eq!(img.to_srgb16()[3], 12345);
    }

    #[test]
    fn downscale_averages_in_linear_light() {
        // A 2x2 block of linear 0.0 and 1.0 averages to 0.5 linear, not the sRGB midpoint.
        let img = LinearImage::new(2, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
        let small = img.downscale(2);
        assert_eq!((small.width(), small.height()), (1, 1));
        assert!((small.data()[0] - 0.5).abs() < 1e-6, "got {}", small.data()[0]);
        assert!((small.data()[3] - 1.0).abs() < 1e-6, "alpha must average too");
    }

    #[test]
    fn downscale_keeps_a_constant_image_constant() {
        let img = LinearImage::new(5, 3, vec![0.25; 5 * 3 * 4]).unwrap();
        let small = img.downscale(4);
        assert_eq!((small.width(), small.height()), (2, 1));
        assert!(small.data().iter().all(|v| (v - 0.25).abs() < 1e-6));
    }

    #[test]
    fn rejects_zero_dimensions_and_short_buffers() {
        assert_eq!(
            LinearImage::new(0, 1, vec![]),
            Err(EngineError::InvalidDimensions { width: 0, height: 1 })
        );
        assert_eq!(
            LinearImage::from_srgb8(2, 2, &[0; 3]),
            Err(EngineError::LengthMismatch { expected: 16, actual: 3 })
        );
    }

    #[test]
    fn non_finite_values_encode_as_zero() {
        let img = LinearImage::new(1, 1, vec![f32::NAN, f32::INFINITY, -1.0, 0.5]).unwrap();
        let out = img.to_srgb8();
        assert_eq!(&out[..3], &[0, 0, 0]);
        assert_eq!(out[3], 128);
    }
}
