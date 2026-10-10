//! C ABI over `engine`. Validates buffers, copies rows, guards panics. No image maths here.

use std::panic::{catch_unwind, AssertUnwindSafe};

use engine::{apply_exposure, LinearImage};

pub const PE_OK: i32 = 0;
pub const PE_NULL: i32 = -1;
pub const PE_BAD_DIMENSIONS: i32 = -2;
pub const PE_BAD_ROWBYTES: i32 = -3;
pub const PE_BAD_PARAMETER: i32 = -4;
pub const PE_PANIC: i32 = -5;

/// Applies global exposure in place to 8-bit sRGB RGBA pixels.
///
/// `row_bytes` is the distance between row starts and must be at least `width * 4`.
/// Bytes in the padding after each row are never read or written.
/// On any error the buffer is left unchanged.
///
/// # Safety
/// `pixels` must be valid for reads and writes of `row_bytes * height` bytes.
#[no_mangle]
pub unsafe extern "C" fn pe_apply_exposure_rgba8(
    pixels: *mut u8,
    width: u32,
    height: u32,
    row_bytes: usize,
    ev: f32,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: forwarded from this function's contract.
        unsafe { apply_exposure_rgba8(pixels, width, height, row_bytes, ev) }
    }))
    .unwrap_or(PE_PANIC)
}

unsafe fn apply_exposure_rgba8(
    pixels: *mut u8,
    width: u32,
    height: u32,
    row_bytes: usize,
    ev: f32,
) -> i32 {
    if pixels.is_null() {
        return PE_NULL;
    }
    if width == 0 || height == 0 {
        return PE_BAD_DIMENSIONS;
    }
    let w = width as usize;
    let h = height as usize;
    let Some(packed_row) = w.checked_mul(4) else {
        return PE_BAD_DIMENSIONS;
    };
    if row_bytes < packed_row {
        return PE_BAD_ROWBYTES;
    }
    let Some(total) = row_bytes.checked_mul(h).filter(|&t| t <= isize::MAX as usize) else {
        return PE_BAD_DIMENSIONS;
    };
    if !ev.is_finite() {
        return PE_BAD_PARAMETER;
    }

    // SAFETY: caller guarantees `total` readable and writable bytes at `pixels`.
    let buf = unsafe { std::slice::from_raw_parts_mut(pixels, total) };

    let mut packed = Vec::with_capacity(packed_row * h);
    for y in 0..h {
        packed.extend_from_slice(&buf[y * row_bytes..y * row_bytes + packed_row]);
    }
    let Ok(mut img) = LinearImage::from_srgb8(w, h, &packed) else {
        return PE_BAD_DIMENSIONS;
    };
    if apply_exposure(&mut img, ev).is_err() {
        return PE_BAD_PARAMETER;
    }
    let out = img.to_srgb8();
    for y in 0..h {
        buf[y * row_bytes..y * row_bytes + packed_row]
            .copy_from_slice(&out[y * packed_row..(y + 1) * packed_row]);
    }
    PE_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x2 image, 12-byte rows (4 bytes of padding per row) filled with 0xAA.
    fn padded_buffer() -> Vec<u8> {
        let mut buf = vec![0xAAu8; 12 * 2];
        let row = [100u8, 100, 100, 255, 100, 100, 100, 255];
        for y in 0..2 {
            buf[y * 12..y * 12 + 8].copy_from_slice(&row);
        }
        buf
    }

    #[test]
    fn zero_ev_leaves_pixels_and_padding_unchanged() {
        let mut buf = padded_buffer();
        let before = buf.clone();
        let rc = unsafe { pe_apply_exposure_rgba8(buf.as_mut_ptr(), 2, 2, 12, 0.0) };
        assert_eq!(rc, PE_OK);
        assert_eq!(buf, before);
    }

    #[test]
    fn positive_ev_changes_pixels_but_never_padding() {
        let mut buf = padded_buffer();
        let rc = unsafe { pe_apply_exposure_rgba8(buf.as_mut_ptr(), 2, 2, 12, 1.0) };
        assert_eq!(rc, PE_OK);
        assert!(buf[0] > 100, "red channel should brighten");
        for y in 0..2 {
            assert_eq!(&buf[y * 12 + 8..y * 12 + 12], &[0xAA; 4], "padding row {y}");
        }
    }

    #[test]
    fn rejects_bad_arguments_without_touching_buffer() {
        let mut buf = padded_buffer();
        let before = buf.clone();
        unsafe {
            assert_eq!(pe_apply_exposure_rgba8(std::ptr::null_mut(), 2, 2, 12, 0.0), PE_NULL);
            assert_eq!(pe_apply_exposure_rgba8(buf.as_mut_ptr(), 0, 2, 12, 0.0), PE_BAD_DIMENSIONS);
            assert_eq!(pe_apply_exposure_rgba8(buf.as_mut_ptr(), 2, 2, 7, 0.0), PE_BAD_ROWBYTES);
            assert_eq!(pe_apply_exposure_rgba8(buf.as_mut_ptr(), 2, 2, 12, f32::NAN), PE_BAD_PARAMETER);
        }
        assert_eq!(buf, before);
    }
}
