//! Quality probe: renders deterministic synthetic fixtures through both engine paths and prints
//! measurements. Metrics are in sRGB code values (0..255) so they match what the user sees.
//!
//! Usage: cargo run -p engine --release --example quality_probe -- <out_dir>
//! Writes the fixtures and renders as PNG under <out_dir> so before/after can be compared.

use std::{env, fs, path::Path};

use engine::limestone_exact::{image_buffer::RgbImageF32, params::AdjustParams, pipeline::process_image};
use engine::{apply_look, details::Details, AdaptiveExposure, LinearImage, Look};
use image::{imageops::FilterType, RgbImage};

type Px = [f32; 3];

fn hash(x: u32, y: u32, s: u32) -> f32 {
    let mut h = x.wrapping_mul(0x27d4_eb2d) ^ y.wrapping_mul(0x1656_67b1) ^ s.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32
}

/// Roughly Gaussian noise in [-1, 1], std about 0.33.
fn noise(x: u32, y: u32, s: u32) -> f32 {
    (hash(x, y, s) + hash(x, y, s + 101) + hash(x, y, s + 202)) * (2.0 / 3.0) - 1.0
}

/// Smooth value noise in [0, 1) with the given lattice cell size in pixels.
fn value_noise(x: f32, y: f32, cell: f32, s: u32) -> f32 {
    let (gx, gy) = (x / cell, y / cell);
    let (ix, iy) = (gx.floor() as i32, gy.floor() as i32);
    let (fx, fy) = (gx - gx.floor(), gy - gy.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let h = |dx: i32, dy: i32| hash((ix + dx) as u32, (iy + dy) as u32, s);
    let top = h(0, 0) + (h(1, 0) - h(0, 0)) * sx;
    let bot = h(0, 1) + (h(1, 1) - h(0, 1)) * sx;
    top + (bot - top) * sy
}

/// Scenes in sRGB-encoded [0, 1]. Each one isolates one behaviour.
fn scene(name: &str, w: usize, h: usize) -> Vec<Px> {
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let px: Px = match name {
                // Neutral ramp, 0 to 1 across the width.
                "ramp" => {
                    let v = x as f32 / (w - 1) as f32;
                    [v, v, v]
                }
                // Bright sky with a dark tree (crown plus trunk) and a textured foreground.
                "backlit" => {
                    if fy >= 0.55 {
                        let t = 0.04 * (noise(x as u32, y as u32, 3));
                        [0.35 + t, 0.42 + t, 0.25 + t]
                    } else {
                        let k = fy / 0.55;
                        let sky = [0.80 + 0.13 * k, 0.86 + 0.04 * k, 0.95 - 0.13 * k];
                        let (cx, cy, r) = (0.45, 0.30, 0.14);
                        let crown = (fx - cx).powi(2) + ((fy - cy) / 1.0).powi(2) < r * r;
                        let trunk = (0.40..0.50).contains(&fx) && (0.15..0.55).contains(&fy);
                        if crown || trunk {
                            [0.12, 0.13, 0.09]
                        } else {
                            sky
                        }
                    }
                }
                // Flat mid-grey with sensor-like noise of about 3 code values.
                "flat_noise" => {
                    let t = 0.5 + 0.035 * noise(x as u32, y as u32, 7);
                    [t, t, t]
                }
                // Five patches: red, skin, teal, neutral, bright orange (highlight clip test), each with slight texture.
                "saturated" => {
                    let base = match x * 5 / w {
                        0 => [0.78, 0.16, 0.12],
                        1 => [0.86, 0.64, 0.50],
                        2 => [0.12, 0.55, 0.59],
                        3 => [0.50, 0.50, 0.50],
                        _ => [0.98, 0.72, 0.30],
                    };
                    let t = 0.01 * noise(x as u32, y as u32, 11);
                    [base[0] + t, base[1] + t, base[2] + t]
                }
                // Photo-like: left-to-right exposure fall-off plus multi-scale texture, warm cast.
                "photo" => {
                    let (xf, yf) = (x as f32, y as f32);
                    let t = 0.5 * value_noise(xf, yf, 24.0, 1)
                        + 0.25 * value_noise(xf, yf, 8.0, 2)
                        + 0.125 * value_noise(xf, yf, 3.0, 3);
                    let v = (0.15 + 0.7 * fx + (t - 0.45) * 0.4).clamp(0.0, 1.0);
                    [v, v * 0.95, v * 0.80]
                }
                // One-pixel checker between 0.2 and 0.8, for preview aliasing.
                "checker" => {
                    let v = if (x + y) % 2 == 0 { 0.2 } else { 0.8 };
                    [v, v, v]
                }
                _ => unreachable!("unknown scene {name}"),
            };
            out.push(px);
        }
    }
    out
}

fn to_rgb8(px: &[Px]) -> Vec<u8> {
    px.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)).collect()
}

fn with_alpha(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect()
}

/// Ours path: full Look through `apply_look`, 8-bit in and out.
fn render_ours(rgb: &[u8], w: usize, h: usize, look: &Look) -> Vec<u8> {
    let mut img = LinearImage::from_srgb8(w, h, &with_alpha(rgb)).unwrap();
    apply_look(&mut img, look).unwrap();
    img.to_srgb8().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
}

/// Limestone path: the exact port the desktop UI uses by default.
fn render_ls(rgb: &[u8], w: usize, h: usize, p: &AdjustParams) -> Vec<u8> {
    let img = RgbImage::from_raw(w as u32, h as u32, rgb.to_vec()).unwrap();
    process_image(&RgbImageF32::from_rgb8(&img), p).to_rgb8().into_raw()
}

fn luma(p: &[u8]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

fn std_of(v: &[f32]) -> f32 {
    let m = v.iter().sum::<f32>() / v.len() as f32;
    (v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len() as f32).sqrt()
}

/// Std of the fine-scale residual (luma minus a 9x9 mean): how much texture survives.
fn texture_std(rgb: &[u8], w: usize, h: usize) -> f32 {
    let y: Vec<f32> = rgb.chunks_exact(3).map(luma).collect();
    let r = 4isize;
    let mut res = Vec::with_capacity(w * h);
    for yy in 0..h as isize {
        for xx in 0..w as isize {
            let (mut s, mut n) = (0.0, 0.0);
            for dy in -r..=r {
                for dx in -r..=r {
                    let (ax, ay) = ((xx + dx).clamp(0, w as isize - 1), (yy + dy).clamp(0, h as isize - 1));
                    s += y[ay as usize * w + ax as usize];
                    n += 1.0;
                }
            }
            res.push(y[yy as usize * w + xx as usize] - s / n);
        }
    }
    std_of(&res)
}

/// Mean (L, C, hue in degrees) in OKLCH over a rectangle, from sRGB code values.
fn oklch_mean(rgb: &[u8], w: usize, x0: usize, y0: usize, s: usize) -> (f32, f32, f32) {
    let (mut l, mut a, mut b) = (0.0f32, 0.0f32, 0.0f32);
    let mut n = 0.0;
    for y in y0..y0 + s {
        for x in x0..x0 + s {
            let i = (y * w + x) * 3;
            let lin = [rgb[i], rgb[i + 1], rgb[i + 2]].map(|c| srgb_to_lin(c as f32 / 255.0));
            let [ll, aa, bb] = to_oklab(lin);
            l += ll;
            a += aa;
            b += bb;
            n += 1.0;
        }
    }
    let (l, a, b) = (l / n, a / n, b / n);
    (l, (a * a + b * b).sqrt(), b.atan2(a).to_degrees())
}

fn srgb_to_lin(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn to_oklab(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c;
    let l = (0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_50 * r + 0.680_699_55 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_70 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_78 * m - 0.004_072_05 * s,
        1.977_998_50 * l - 2.428_592_20 * m + 0.450_593_71 * s,
        0.025_904_04 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// Fraction of pixels in a rectangle where any channel is at 99% or more.
fn clip_fraction(rgb: &[u8], w: usize, x0: usize, y0: usize, s: usize) -> f32 {
    let mut n = 0;
    let mut hit = 0;
    for y in y0..y0 + s {
        for x in x0..x0 + s {
            let i = (y * w + x) * 3;
            n += 1;
            if rgb[i..i + 3].iter().any(|&c| c >= 252) {
                hit += 1;
            }
        }
    }
    hit as f32 / n as f32
}

/// Halo energy around a vertical edge at column `x0`, on row `y`. Measures how much the
/// filter bends the profile away from its own plateau, relative to the input.
fn halo(before: &[u8], after: &[u8], w: usize, y: usize, x0: usize, outside: bool) -> f32 {
    let at = |img: &[u8], x: usize| luma(&img[(y * w + x) * 3..]);
    let (band, plateau): (Vec<usize>, usize) = if outside {
        ((x0 - 10..x0).collect(), x0 - 40)
    } else {
        ((x0 + 1..x0 + 10).collect(), x0 + 25)
    };
    let dev = |img: &[u8], x: usize| at(img, x) - at(img, plateau);
    band.iter()
        .map(|&x| (dev(after, x) - dev(before, x)).abs())
        .fold(0.0, f32::max)
}

fn print_row(label: &str, value: String) {
    println!("  {label:<46} {value}");
}

fn run(out_dir: &Path, label: &str, w: usize, h: usize) {
    let dir = out_dir.join(label);
    fs::create_dir_all(&dir).unwrap();
    let save = |name: &str, rgb: &[u8], w: usize, h: usize| {
        RgbImage::from_raw(w as u32, h as u32, rgb.to_vec()).unwrap().save(dir.join(name)).unwrap();
    };

    let scenes = ["ramp", "backlit", "flat_noise", "saturated", "photo", "checker"];
    let mut inputs = std::collections::HashMap::new();
    for s in scenes {
        let rgb = to_rgb8(&scene(s, w, h));
        save(&format!("input_{s}.png"), &rgb, w, h);
        inputs.insert(s, rgb);
    }

    // Condition A: adaptive exposure at full strength (the user's "Detail - Medium" readout uses 0.57).
    let ls_adapt = |p: &mut AdjustParams| {
        p.adaptive_exposure.enabled = true;
        p.adaptive_exposure.adaptive_exposure = 1.0;
        p.adaptive_exposure.regions = 10;
    };
    let ours_adapt = |l: &mut Look| {
        l.adaptive = AdaptiveExposure { strength: 1.0, regions: 4, ..Default::default() };
    };
    // Condition B: details with the user's "Detail - Medium" readouts (strength 1.07, boost 1.30,
    // threshold 0.08, radius 30, sharpen 1.00).
    let ls_detail = |p: &mut AdjustParams| {
        p.details.enabled = true;
        p.details.strength = 1.07;
        p.details.detail_boost = 1.30;
        p.details.threshold = 0.08;
        p.details.radius = 30.0;
        p.details.sharpen = 1.0;
        p.details.process_independently = false;
    };
    let ours_detail = |l: &mut Look| {
        l.details = Details { strength: 1.07, boost: 1.30, threshold: 0.08, radius: 16.0, sharpen: 1.0, process_independently: false };
    };

    println!("\n## {label} ({w}x{h})");

    // ---- Limestone path (what the desktop UI shows by default) ----
    let mut p = AdjustParams::default();
    ls_adapt(&mut p);
    let ls_a = |s: &str| render_ls(&inputs[s], w, h, &p);
    let back = ls_a("backlit");
    save("ls_adaptive_backlit.png", &back, w, h);
    let ramp = ls_a("ramp");
    save("ls_adaptive_ramp.png", &ramp, w, h);
    println!("Limestone path, adaptive exposure = 1.0, regions = 10");
    let tree_x = (0.31 * w as f32) as usize;
    let row = (0.30 * h as f32) as usize;
    print_row("halo, sky side of tree edge (code values)", format!("{:.2}", halo(&inputs["backlit"], &back, w, row, tree_x, true) * 1.0));
    print_row("halo, crown side of tree edge (code values)", format!("{:.2}", halo(&inputs["backlit"], &back, w, row, tree_x, false)));
    let sat = &inputs["saturated"];
    let sat_out = ls_a("saturated");
    save("ls_adaptive_saturated.png", &sat_out, w, h);
    for (i, name) in ["red", "skin", "teal", "grey", "bright_orange"].iter().enumerate() {
        let x0 = w / 5 * i + w / 10 - 8;
        let (_, c0, h0) = oklch_mean(sat, w, x0, h / 2 - 8, 16);
        let (_, c1, h1) = oklch_mean(&sat_out, w, x0, h / 2 - 8, 16);
        let dh = (h1 - h0 + 540.0).rem_euclid(360.0) - 180.0;
        print_row(
            &format!("patch {name}: hue shift / chroma ratio / new clip"),
            format!("{dh:+6.2} deg / {:.3} / {:.3}", c1 / c0.max(1e-6), clip_fraction(&sat_out, w, x0, h / 2 - 8, 16) - clip_fraction(sat, w, x0, h / 2 - 8, 16)),
        );
    }
    if env::var("PROBE_DEBUG").is_ok() {
        for x in (tree_x - 12..tree_x + 12).step_by(2) {
            let i = (row * w + x) * 3;
            println!("    x={x} before={:?} ls={:?}", &inputs["backlit"][i..i + 3], &back[i..i + 3]);
        }
    }
    let r = &ramp;
    let spread = r.chunks_exact(3).map(|c| c.iter().max().unwrap() - c.iter().min().unwrap()).max().unwrap();
    let nonmono = (1..w).filter(|&x| luma(&r[x * 3..]) + 0.5 < luma(&r[(x - 1) * 3..])).count();
    print_row("ramp: max R-G-B spread / non-monotone steps", format!("{spread} / {nonmono}"));

    let mut pd = AdjustParams::default();
    ls_detail(&mut pd);
    let flat = render_ls(&inputs["flat_noise"], w, h, &pd);
    save("ls_detail_flat_noise.png", &flat, w, h);
    let cx = (w / 2 - 32, h / 2 - 32);
    let sd = |img: &[u8]| {
        let v: Vec<f32> = (0..64 * 64).map(|i| luma(&img[((cx.1 + i / 64) * w + cx.0 + i % 64) * 3..])).collect();
        std_of(&v)
    };
    print_row("detail: flat-patch noise ratio (after/before)", format!("{:.2}", sd(&flat) / sd(&inputs["flat_noise"])));
    let bd = render_ls(&inputs["backlit"], w, h, &pd);
    save("ls_detail_backlit.png", &bd, w, h);
    print_row("detail: halo, sky side (code values)", format!("{:.2}", halo(&inputs["backlit"], &bd, w, row, tree_x, true)));
    print_row("detail: halo, crown side (code values)", format!("{:.2}", halo(&inputs["backlit"], &bd, w, row, tree_x, false)));

    let photo = &inputs["photo"];
    let pa = render_ls(photo, w, h, &p);
    save("ls_adaptive_photo.png", &pa, w, h);
    let lum_std = |img: &[u8]| std_of(&img.chunks_exact(3).map(luma).collect::<Vec<_>>());
    print_row("adaptive: tonal range ratio (std after/before)", format!("{:.2}", lum_std(&pa) / lum_std(photo)));
    print_row("adaptive: texture retention (fine residual std ratio)", format!("{:.2}", texture_std(&pa, w, h) / texture_std(photo, w, h)));

    // Preview vs export: the app shrinks the input (Triangle) for preview and renders full for export.
    let full_w = w * 2;
    let full_h = h * 2;
    let big = to_rgb8(&scene("photo", full_w, full_h));
    let big_img = RgbImage::from_raw(full_w as u32, full_h as u32, big.clone()).unwrap();
    let export = render_ls(&big, full_w, full_h, &pd);
    let export_small = image::imageops::resize(&RgbImage::from_raw(full_w as u32, full_h as u32, export).unwrap(), w as u32, h as u32, FilterType::Triangle);
    let small_in = image::imageops::resize(&big_img, w as u32, h as u32, FilterType::Triangle);
    let preview = render_ls(small_in.as_raw(), w, h, &pd);
    let mad = export_small.as_raw().iter().zip(&preview).map(|(a, b)| (*a as f32 - *b as f32).abs()).sum::<f32>() / preview.len() as f32;
    print_row("preview vs export, detail (mean abs code diff)", format!("{mad:.2}"));

    // ---- Ours path (the native engine used by render-cli) ----
    let mut look = Look::default();
    ours_adapt(&mut look);
    let back_o = render_ours(&inputs["backlit"], w, h, &look);
    save("ours_adaptive_backlit.png", &back_o, w, h);
    let photo_o = render_ours(photo, w, h, &look);
    save("ours_adaptive_photo.png", &photo_o, w, h);
    println!("Ours path, adaptive exposure = 1.0, regions = 4");
    print_row("halo, sky side of tree edge (code values)", format!("{:.2}", halo(&inputs["backlit"], &back_o, w, row, tree_x, true)));
    print_row("halo, crown side of tree edge (code values)", format!("{:.2}", halo(&inputs["backlit"], &back_o, w, row, tree_x, false)));
    print_row("adaptive: tonal range ratio (std after/before)", format!("{:.2}", lum_std(&photo_o) / lum_std(photo)));
    print_row("adaptive: texture retention (fine residual std ratio)", format!("{:.2}", texture_std(&photo_o, w, h) / texture_std(photo, w, h)));

    let mut look_d = Look::default();
    ours_detail(&mut look_d);
    let flat_o = render_ours(&inputs["flat_noise"], w, h, &look_d);
    save("ours_detail_flat_noise.png", &flat_o, w, h);
    println!("Ours path, details (strength 1.07, boost 1.30, sharpen 1.0, radius 16)");
    print_row("detail: flat-patch noise ratio (after/before)", format!("{:.2}", sd(&flat_o) / sd(&inputs["flat_noise"])));
    let bd_o = render_ours(&inputs["backlit"], w, h, &look_d);
    save("ours_detail_backlit.png", &bd_o, w, h);
    print_row("detail: halo, sky side (code values)", format!("{:.2}", halo(&inputs["backlit"], &bd_o, w, row, tree_x, true)));
    print_row("detail: halo, crown side (code values)", format!("{:.2}", halo(&inputs["backlit"], &bd_o, w, row, tree_x, false)));
    let ramp_o = render_ours(&inputs["ramp"], w, h, &look);
    let spread_o = ramp_o.chunks_exact(3).map(|c| c.iter().max().unwrap() - c.iter().min().unwrap()).max().unwrap();
    print_row("ramp: max R-G-B spread", format!("{spread_o}"));
    let sat_o = render_ours(sat, w, h, &look);
    save("ours_adaptive_saturated.png", &sat_o, w, h);
    for (i, name) in ["red", "skin", "teal", "grey", "bright_orange"].iter().enumerate() {
        let x0 = w / 5 * i + w / 10 - 8;
        let (_, c0, h0) = oklch_mean(sat, w, x0, h / 2 - 8, 16);
        let (_, c1, h1) = oklch_mean(&sat_o, w, x0, h / 2 - 8, 16);
        let dh = (h1 - h0 + 540.0).rem_euclid(360.0) - 180.0;
        print_row(
            &format!("patch {name}: hue shift / chroma ratio"),
            format!("{dh:+6.2} deg / {:.3} / chroma {:.4}->{:.4}", c1 / c0.max(1e-6), c0, c1),
        );
    }

    // Native preview check, same question: shrink the input, render, compare with shrunk export.
    let exp_o = render_ours(&big, full_w, full_h, &look_d);
    let exp_small = image::imageops::resize(&RgbImage::from_raw(full_w as u32, full_h as u32, exp_o).unwrap(), w as u32, h as u32, FilterType::Triangle);
    let prev_o = render_ours(small_in.as_raw(), w, h, &look_d);
    let mad_o = exp_small.as_raw().iter().zip(&prev_o).map(|(a, b)| (*a as f32 - *b as f32).abs()).sum::<f32>() / prev_o.len() as f32;
    print_row("preview vs export, detail (mean abs code diff)", format!("{mad_o:.2}"));

    // Checker: how much a preview shrinks the highest-frequency content.
    let ck = inputs["checker"].clone();
    let ck_out = render_ls(&ck, w, h, &pd);
    let ck_std = std_of(&ck_out.chunks_exact(3).map(luma).collect::<Vec<_>>());
    print_row("detail: checker residual std (code values)", format!("{ck_std:.2}"));
}

/// Usage: quality_probe <out_dir> <label>. Renders go to <out_dir>/<label>/.
fn main() {
    let args: Vec<String> = env::args().collect();
    let out_dir = Path::new(args.get(1).map(String::as_str).unwrap_or("quality"));
    let label = args.get(2).map(String::as_str).unwrap_or("run");
    run(out_dir, label, 256, 256);
}
