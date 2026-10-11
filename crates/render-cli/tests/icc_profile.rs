//! Embedded ICC profiles are converted to sRGB on load. The profile is read from the system at test
//! time (Apple ships it; it is not copied into the repo). If it is missing, the test is skipped.

use std::process::Command;

const P3_PATH: &str = "/System/Library/ColorSync/Profiles/Display P3.icc";

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// zlib stream with stored (uncompressed) deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = data.chunks(65535).collect();
    for (i, c) in chunks.iter().enumerate() {
        let last = i + 1 == chunks.len();
        out.push(if last { 1 } else { 0 });
        let len = c.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
    out
}

/// PNG of a single RGB colour, optionally tagged with an ICC profile in an iCCP chunk.
fn png_with_profile(path: &std::path::Path, rgb: [u8; 3], icc: Option<&[u8]>) {
    let mut plain = Vec::new();
    image::RgbImage::from_pixel(16, 16, image::Rgb(rgb))
        .write_to(&mut std::io::Cursor::new(&mut plain), image::ImageFormat::Png)
        .unwrap();
    let Some(icc) = icc else {
        std::fs::write(path, plain).unwrap();
        return;
    };
    // Insert iCCP right after IHDR (8-byte signature, then the 25-byte IHDR chunk).
    let split = 8 + 25;
    let mut iccp = b"Display P3".to_vec();
    iccp.push(0);
    iccp.push(0);
    iccp.extend_from_slice(&zlib_stored(icc));
    let mut out = plain[..split].to_vec();
    out.extend_from_slice(&png_chunk(b"iCCP", &iccp));
    out.extend_from_slice(&plain[split..]);
    std::fs::write(path, out).unwrap();
}

fn run(input: &std::path::Path, output: &std::path::Path) -> image::RgbaImage {
    let status = Command::new(env!("CARGO_BIN_EXE_render-cli")).arg(input).arg(output).status().unwrap();
    assert!(status.success());
    image::open(output).unwrap().to_rgba8()
}

#[test]
fn display_p3_is_converted_to_srgb() {
    let Ok(icc) = std::fs::read(P3_PATH) else {
        eprintln!("Display P3 profile not found: skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("icc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let colour = [60u8, 200, 90];

    // Same pixels, untagged: the engine takes them as sRGB, so they come back unchanged.
    let plain_in = dir.join("plain.png");
    png_with_profile(&plain_in, colour, None);
    let plain = run(&plain_in, &dir.join("plain_out.png"));
    assert_eq!(plain.get_pixel(0, 0).0[..3], colour, "untagged image must round-trip");

    // Same pixels, tagged Display P3: the output must move to sRGB values.
    let tagged_in = dir.join("p3.png");
    png_with_profile(&tagged_in, colour, Some(&icc));
    let tagged = run(&tagged_in, &dir.join("p3_out.png"));
    let p = tagged.get_pixel(0, 0).0;
    let moved = (0..3).any(|c| (p[c] as i32 - colour[c] as i32).abs() > 3);
    assert!(moved, "P3 profile was not applied: {:?} -> {:?}", colour, &p[..3]);
    let _ = std::fs::remove_dir_all(&dir);
}
