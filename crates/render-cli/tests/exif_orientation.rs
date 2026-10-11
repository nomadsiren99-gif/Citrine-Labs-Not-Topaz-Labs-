//! EXIF orientation is applied on load: a stored 8x4 image tagged orientation 6 (rotate 90 degrees
//! clockwise) must come out 4x8, with the stored left (red) half now along the top.

use std::process::Command;

#[test]
fn exif_orientation_six_rotates_on_load() {
    let dir = std::env::temp_dir().join(format!("exif_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("out.png");
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/orient6.jpg");
    let status = Command::new(env!("CARGO_BIN_EXE_render-cli")).arg(fixture).arg(&out).status().unwrap();
    assert!(status.success());
    let img = image::open(&out).unwrap().to_rgb8();
    assert_eq!(img.dimensions(), (4, 8), "orientation was not applied");
    let top = img.get_pixel(0, 0).0;
    assert!(top[0] > 200 && top[2] < 60, "top-left should be red after rotation, got {top:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
