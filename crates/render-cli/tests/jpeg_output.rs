//! JPEG output writes RGB (no alpha) and keeps the size, so a JPEG in gives a JPEG out.

use std::process::Command;

#[test]
fn jpeg_output_is_rgb_and_keeps_size() {
    let dir = std::env::temp_dir().join(format!("jpgout_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/orient6.jpg");
    let out = dir.join("out.jpg");
    let status = Command::new(env!("CARGO_BIN_EXE_render-cli")).arg(input).arg(&out).status().unwrap();
    assert!(status.success());
    let decoded = image::open(&out).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (4, 8));
    assert_eq!(decoded.color(), image::ColorType::Rgb8);
    let _ = std::fs::remove_dir_all(&dir);
}
