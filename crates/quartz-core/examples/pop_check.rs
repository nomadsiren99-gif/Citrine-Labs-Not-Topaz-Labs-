//! Renders one upstream factory preset on an image and prints the skin colour of the face.
//! Usage: pop_check <image> <out.png> <preset name>

use adjust_core::{get_factory_presets, process_image, RgbImageF32};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = image::open(&args[1]).unwrap().to_rgb8();
    let preset = get_factory_presets().into_iter().find(|p| p.name == args[3]).expect("no such preset");
    let out = process_image(&RgbImageF32::from_rgb8(&src), &preset.params).to_rgb8();
    out.save(&args[2]).unwrap();
    let (w, h) = (out.width() as usize, out.height() as usize);
    let (mut r, mut g, mut b, mut n) = (0f64, 0f64, 0f64, 0f64);
    for y in h * 25 / 100..h * 55 / 100 {
        for x in w * 35 / 100..w * 65 / 100 {
            let p = out.get_pixel(x as u32, y as u32).0;
            r += p[0] as f64;
            g += p[1] as f64;
            b += p[2] as f64;
            n += 1.0;
        }
    }
    println!("{:<22} R-G {:+6.1}  B-G {:+6.1}", args[3], (r - g) / n, (b - g) / n);
}
