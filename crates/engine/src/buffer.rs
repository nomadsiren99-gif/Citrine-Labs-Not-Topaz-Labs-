use std::fmt;

use crate::color::{linear_to_srgb, srgb_to_linear};

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    InvalidDimensions { width: usize, height: usize },
    LengthMismatch { expected: usize, actual: usize },
    NonFiniteParameter(&'static str),
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
        let data = rgba
            .chunks_exact(4)
            .flat_map(|p| {
                [
                    srgb_to_linear(p[0] as f32 / 255.0),
                    srgb_to_linear(p[1] as f32 / 255.0),
                    srgb_to_linear(p[2] as f32 / 255.0),
                    p[3] as f32 / 255.0,
                ]
            })
            .collect();
        Ok(Self { width, height, data })
    }

    /// Encodes to 8-bit sRGB RGBA. Non-finite values become 0; out-of-range values are clamped.
    pub fn to_srgb8(&self) -> Vec<u8> {
        self.data
            .chunks_exact(4)
            .flat_map(|p| {
                [
                    encode_channel(p[0]),
                    encode_channel(p[1]),
                    encode_channel(p[2]),
                    to_unit_u8(p[3]),
                ]
            })
            .collect()
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
