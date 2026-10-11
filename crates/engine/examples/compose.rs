//! Side-by-side gallery builder for quality checks. Places the same crop of each image next to
//! the others, scaled by an integer factor (nearest, so pixels stay exact).
//!
//! Usage: cargo run -p engine --release --example compose -- <out.png> <scale> <x> <y> <w> <h> <img>...
//! Pass w = 0 and h = 0 to use the whole image.

use std::env;

use image::RgbImage;

fn main() {
    let a: Vec<String> = env::args().collect();
    if a.len() < 8 {
        eprintln!("usage: compose <out.png> <scale> <x> <y> <w> <h> <img>...");
        std::process::exit(2);
    }
    let out = &a[1];
    let scale: u32 = a[2].parse().unwrap();
    let (x, y, cw, ch): (u32, u32, u32, u32) = (a[3].parse().unwrap(), a[4].parse().unwrap(), a[5].parse().unwrap(), a[6].parse().unwrap());
    let imgs: Vec<RgbImage> = a[7..].iter().map(|p| image::open(p).unwrap().to_rgb8()).collect();
    let cw = if cw == 0 { imgs[0].width() } else { cw };
    let ch = if ch == 0 { imgs[0].height() } else { ch };
    let gap = 8 * scale;
    let total_w = imgs.len() as u32 * (cw * scale) + (imgs.len() as u32 - 1) * gap;
    let mut canvas = RgbImage::from_pixel(total_w, ch * scale, image::Rgb([255, 255, 255]));
    for (i, img) in imgs.iter().enumerate() {
        let ox = i as u32 * (cw * scale + gap);
        for dy in 0..ch * scale {
            for dx in 0..cw * scale {
                let p = img.get_pixel(x + dx / scale, y + dy / scale);
                canvas.put_pixel(ox + dx, dy, *p);
            }
        }
    }
    canvas.save(out).unwrap();
    println!("wrote {out} ({}x{})", canvas.width(), canvas.height());
}
