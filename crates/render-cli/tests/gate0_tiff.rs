//! Gate 0: a neutral render must return the input unchanged, at 16-bit depth, alpha included.

use std::process::Command;

#[test]
fn neutral_16_bit_tiff_round_trips_exactly() {
    let dir = std::env::temp_dir().join(format!("gate0_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("in.tif");
    let output = dir.join("out.tif");

    // 256x256 RGBA16 with every code value in the red channel and varied green, blue, alpha.
    let mut px = Vec::with_capacity(256 * 256 * 4);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let r = (x * 257) as u16;
            let g = (y * 257) as u16;
            let b = ((x * 31 + y * 17) % 65536) as u16;
            let a = (65535 - x * 200) as u16;
            px.extend_from_slice(&[r, g, b, a]);
        }
    }
    image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(256, 256, px.clone())
        .unwrap()
        .save(&input)
        .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_render-cli"))
        .arg(&input)
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());

    let back = image::open(&output).unwrap();
    assert_eq!(back.color(), image::ColorType::Rgba16, "output lost its 16-bit depth");
    let back = back.to_rgba16().into_raw();
    let bad = back.iter().zip(&px).filter(|(a, b)| a != b).count();
    assert_eq!(bad, 0, "{bad} of {} samples changed", px.len());
    let _ = std::fs::remove_dir_all(&dir);
}
