//! Renders the Quartz factory presets whose names contain "Pop" on one image, and prints the
//! skin colour of the face. Usage: quartz_pop_check <image> <out_dir>

use engine::quartz_exact::{get_factory_presets, image_buffer::RgbImageF32, pipeline::process_image};
use image::RgbImage;
use std::path::Path;

fn face_rg(img: &RgbImage) -> (f32, f32) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let (mut r, mut g, mut b, mut n) = (0f64, 0f64, 0f64, 0f64);
    for y in h * 25 / 100..h * 55 / 100 {
        for x in w * 35 / 100..w * 65 / 100 {
            let p = img.get_pixel(x as u32, y as u32).0;
            r += p[0] as f64;
            g += p[1] as f64;
            b += p[2] as f64;
            n += 1.0;
        }
    }
    (((r - g) / n) as f32, ((b - g) / n) as f32)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = image::open(&args[1]).unwrap().to_rgb8();
    let out = Path::new(&args[2]);
    std::fs::create_dir_all(out).unwrap();
    let input = RgbImageF32::from_rgb8(&src);
    let (r0, b0) = face_rg(&src);
    println!("{:<22} R-G {:+6.1}  B-G {:+6.1}", "original", r0, b0);
    for p in get_factory_presets().iter().filter(|p| p.name.contains("Pop")) {
        let img = process_image(&input, &p.params).to_rgb8();
        let (r, b) = face_rg(&img);
        println!("{:<22} R-G {:+6.1}  B-G {:+6.1}", p.name, r, b);
        img.save(out.join(format!("{}.png", p.name.replace(' ', "_")))).unwrap();
    }
    src.save(out.join("original.png")).unwrap();
}
