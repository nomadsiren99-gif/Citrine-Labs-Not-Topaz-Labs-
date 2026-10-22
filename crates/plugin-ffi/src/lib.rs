//! C ABI for the Photoshop plug-in: renders RGB8 pixels with a Limestone parameter set given as JSON.
//! Guards panics and validates every argument. No image maths here.

use std::panic::{catch_unwind, AssertUnwindSafe};


pub const PE_OK: i32 = 0;
pub const PE_NULL: i32 = -1;
pub const PE_BAD_DIMENSIONS: i32 = -2;
pub const PE_BAD_PARAMETER: i32 = -4;
pub const PE_PANIC: i32 = -5;

/// Renders 8-bit RGB pixels in place with an upstream Limestone parameter set given as JSON.
///
/// `pixels` holds `width * height * 3` bytes, tightly packed. `params` is the JSON text of
/// `adjust_core::params::AdjustParams`. Panics are caught here and never cross the C ABI.
/// On any error the buffer is left unchanged.
///
/// # Safety
/// `pixels` must be valid for reads and writes of `width * height * 3` bytes, and `params` for
/// `params_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn citrine_render_rgb8(
    pixels: *mut u8,
    width: u32,
    height: u32,
    params: *const u8,
    params_len: usize,
) -> i32 {
    if pixels.is_null() || params.is_null() {
        return PE_NULL;
    }
    let Some(len) = (width as usize).checked_mul(height as usize).and_then(|p| p.checked_mul(3)) else {
        return PE_BAD_DIMENSIONS;
    };
    if width == 0 || height == 0 {
        return PE_BAD_DIMENSIONS;
    }
    let json = std::slice::from_raw_parts(params, params_len);
    let Ok(parsed) = serde_json::from_slice::<adjust_core::params::AdjustParams>(json) else {
        return PE_BAD_PARAMETER;
    };
    let buf = std::slice::from_raw_parts_mut(pixels, len);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let img = image::RgbImage::from_raw(width, height, buf.to_vec())?;
        let out = adjust_core::process_image(&adjust_core::RgbImageF32::from_rgb8(&img), &parsed).to_rgb8();
        Some(out.into_raw())
    }));
    match result {
        Ok(Some(out)) if out.len() == len => {
            buf.copy_from_slice(&out);
            PE_OK
        }
        Ok(_) => PE_BAD_DIMENSIONS,
        Err(_) => PE_PANIC,
    }
}

/// Renders floating-point RGB in place: `pixels` holds `width * height * 3` values in 0..1, in row order.
/// Used for 16-bit layers, where the values are not rounded to 8 bits before the pipeline runs.
/// On any error the buffer is left unchanged.
///
/// # Safety
/// `pixels` must be valid for reads and writes of `width * height * 3` f32 values, and `params` for
/// `params_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn citrine_render_f32(
    pixels: *mut f32,
    width: u32,
    height: u32,
    params: *const u8,
    params_len: usize,
) -> i32 {
    if pixels.is_null() || params.is_null() {
        return PE_NULL;
    }
    if width == 0 || height == 0 {
        return PE_BAD_DIMENSIONS;
    }
    let Some(len) = (width as usize).checked_mul(height as usize).and_then(|p| p.checked_mul(3)) else {
        return PE_BAD_DIMENSIONS;
    };
    let json = std::slice::from_raw_parts(params, params_len);
    let Ok(parsed) = serde_json::from_slice::<adjust_core::params::AdjustParams>(json) else {
        return PE_BAD_PARAMETER;
    };
    let buf = std::slice::from_raw_parts_mut(pixels, len);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let (w, h) = (width as usize, height as usize);
        let mut input = adjust_core::RgbImageF32::new(w, h);
        for i in 0..w * h {
            input.r[i] = buf[i * 3].clamp(0.0, 1.0);
            input.g[i] = buf[i * 3 + 1].clamp(0.0, 1.0);
            input.b[i] = buf[i * 3 + 2].clamp(0.0, 1.0);
        }
        let out = adjust_core::process_image(&input, &parsed);
        let mut packed = Vec::with_capacity(len);
        for i in 0..w * h {
            packed.extend_from_slice(&[out.r[i], out.g[i], out.b[i]]);
        }
        packed
    }));
    match result {
        Ok(out) if out.len() == len => {
            buf.copy_from_slice(&out);
            PE_OK
        }
        Ok(_) => PE_BAD_DIMENSIONS,
        Err(_) => PE_PANIC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adjust_core::params::AdjustParams;

    #[test]
    fn renders_default_params_in_place_and_keeps_size() {
        let json = serde_json::to_string(&AdjustParams::default()).unwrap();
        let mut buf = vec![120u8; 4 * 3 * 3];
        let rc = unsafe { citrine_render_rgb8(buf.as_mut_ptr(), 4, 3, json.as_ptr(), json.len()) };
        assert_eq!(rc, PE_OK);
        assert_eq!(buf.len(), 4 * 3 * 3);
    }

    #[test]
    fn bad_json_leaves_buffer_unchanged() {
        let mut buf = vec![120u8; 4 * 3 * 3];
        let before = buf.clone();
        let json = b"{not json";
        let rc = unsafe { citrine_render_rgb8(buf.as_mut_ptr(), 4, 3, json.as_ptr(), json.len()) };
        assert_eq!(rc, PE_BAD_PARAMETER);
        assert_eq!(buf, before);
    }

    #[test]
    fn zero_size_is_rejected() {
        let json = b"{}";
        let mut buf = vec![0u8; 3];
        let rc = unsafe { citrine_render_rgb8(buf.as_mut_ptr(), 0, 1, json.as_ptr(), json.len()) };
        assert_eq!(rc, PE_BAD_DIMENSIONS);
    }
}

#[cfg(test)]
mod f32_tests {
    use super::*;

    #[test]
    fn float_render_keeps_precision_beyond_eight_bits() {
        let json = serde_json::to_string(&adjust_core::params::AdjustParams::default()).unwrap();
        // Values that are not multiples of 1/255: an 8-bit round trip would move them.
        let mut buf: Vec<f32> = (0..4 * 3 * 3).map(|i| 0.1234 + i as f32 * 0.001).collect();
        let rc = unsafe { citrine_render_f32(buf.as_mut_ptr(), 4, 3, json.as_ptr(), json.len()) };
        assert_eq!(rc, PE_OK);
        assert!(buf.iter().any(|v| (v * 255.0 - (v * 255.0).round()).abs() > 0.05), "fractional levels survive");
    }

    #[test]
    fn float_render_rejects_bad_json_without_touching_buffer() {
        let mut buf = vec![0.5f32; 3];
        let before = buf.clone();
        let json = b"{nope";
        let rc = unsafe { citrine_render_f32(buf.as_mut_ptr(), 1, 1, json.as_ptr(), json.len()) };
        assert_eq!(rc, PE_BAD_PARAMETER);
        assert_eq!(buf, before);
    }
}
