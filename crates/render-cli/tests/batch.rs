//! Batch mode: a folder in gives one output per image, with the same name, in the output folder.

use std::process::Command;

#[test]
fn folder_input_renders_every_image() {
    let root = std::env::temp_dir().join(format!("batch_{}", std::process::id()));
    let input = root.join("in");
    let output = root.join("out");
    std::fs::create_dir_all(&input).unwrap();
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/orient6.jpg");
    std::fs::copy(fixture, input.join("a.jpg")).unwrap();
    std::fs::copy(fixture, input.join("b.jpg")).unwrap();
    std::fs::write(input.join("notes.txt"), "not an image").unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_render-cli")).arg(&input).arg(&output).status().unwrap();
    assert!(status.success());
    assert!(output.join("a.png").exists() && output.join("b.png").exists(), "both images should be rendered");
    assert!(!output.join("notes.png").exists(), "non-images must be skipped");
    let _ = std::fs::remove_dir_all(&root);
}
