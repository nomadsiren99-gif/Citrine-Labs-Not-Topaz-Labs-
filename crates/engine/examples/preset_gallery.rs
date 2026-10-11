//! Gate 6 harness: renders every candidate recipe from the spec table on the stock photos, writes
//! per-photo contact sheets, and prints automatic metrics. Visual grading is still done by eye.
//!
//! Recipes use neutral IDs and objective descriptions, not the legacy names from the spec table.
//! Mapping from the spec's normalised UI values (proposed, not verified against Topaz):
//!   AE, Ctr, HL, SH, ASat, Sat, SatBoost, Warm, Vig, Grain, Diff : value / 100
//!   Det : strength = 1 + Det/100 (neutral 1)      Boost : boost = 1 + Boost/100 (neutral 1)
//!   ExpBright : brightness_ev = value/100          Hue : degrees
//!   Reg : regions                                  Tonal : not implemented (no tone colours)
//! Vignette: positive darkens, negative is not supported by the engine and is clamped to 0.
//!
//! Usage: cargo run -p engine --release --example preset_gallery -- <out_dir> <photo>...

use std::{env, fs, path::Path};

use engine::{apply_look, AdaptiveExposure, LinearImage, Look};
use engine::details::Details;
use image::{imageops::FilterType, RgbImage};

struct Recipe {
    id: &'static str,
    what: &'static str,
    ae: f32,
    reg: u32,
    ctr: f32,
    exp_bright: f32,
    hl: f32,
    sh: f32,
    det: f32,
    boost: f32,
    asat: f32,
    sat: f32,
    sat_boost: f32,
    hue: f32,
    warm: f32,
    vig: f32,
    grain: f32,
    diff: f32,
    tonal_unimplemented: bool,
}

const fn r(id: &'static str, what: &'static str) -> Recipe {
    Recipe {
        id,
        what,
        ae: 0.0,
        reg: 10,
        ctr: 0.0,
        exp_bright: 0.0,
        hl: 0.0,
        sh: 0.0,
        det: 0.0,
        boost: 0.0,
        asat: 0.0,
        sat: 0.0,
        sat_boost: 0.0,
        hue: 0.0,
        warm: 0.0,
        vig: 0.0,
        grain: 0.0,
        diff: 0.0,
        tonal_unimplemented: false,
    }
}

fn recipes() -> Vec<Recipe> {
    vec![
        Recipe { ae: 16.0, reg: 8, hl: 24.0, sh: 24.0, ..r("R01", "backlight recovery") },
        Recipe { ae: 18.0, reg: 10, det: 12.0, boost: 8.0, asat: 10.0, ..r("R02", "everyday improvement") },
        Recipe { ae: 24.0, ctr: 10.0, asat: 18.0, sat: 5.0, ..r("R03", "wider tone and colour") },
        Recipe { ctr: 18.0, det: 5.0, ..r("R04", "crisp subtle contrast") },
        Recipe { asat: 18.0, sat: 4.0, sat_boost: 6.0, ..r("R05", "soft colour lift") },
        Recipe { det: -25.0, boost: -30.0, diff: 8.0, ..r("R06", "gentler skin") },
        Recipe { ae: 20.0, ctr: 18.0, det: 14.0, sat: -4.0, vig: 16.0, ..r("R07", "moody portrait") },
        Recipe { ctr: 24.0, hl: 15.0, sh: 10.0, ..r("R08", "shadow and light separation") },
        Recipe { det: 18.0, boost: 12.0, ctr: 12.0, ..r("R09", "surface clarity") },
        Recipe { ae: 38.0, ctr: 24.0, det: 28.0, boost: 18.0, sat: -12.0, vig: 20.0, ..r("R10", "controlled HDR contrast") },
        Recipe { det: 8.0, boost: 32.0, reg: 18, ..r("R11", "fine texture") },
        Recipe { det: 22.0, boost: 10.0, reg: 9, ..r("R12", "mid-scale texture") },
        Recipe { ae: 16.0, det: 32.0, boost: 22.0, ..r("R13", "detail respecting noise") },
        Recipe { det: -35.0, boost: -25.0, diff: 12.0, ..r("R14", "smooth simplification") },
        Recipe { det: -28.0, boost: -25.0, ctr: -20.0, sat: -5.0, ..r("R15", "flattened texture") },
        Recipe { ae: 45.0, reg: 14, det: 34.0, boost: 23.0, asat: 24.0, sat: 7.0, hl: 35.0, sh: 25.0, ..r("R16", "hyperreal pseudo-HDR") },
        Recipe { asat: 35.0, sat_boost: 20.0, ..r("R17", "muted-colour presence") },
        Recipe { asat: 55.0, sat: 40.0, sat_boost: 30.0, hue: 12.0, ..r("R18", "surreal colour") },
        Recipe { ae: 35.0, ctr: -8.0, hl: 32.0, sh: 30.0, ..r("R19", "balanced lighting") },
        Recipe { asat: 48.0, sat: 18.0, sat_boost: 26.0, ..r("R20", "very saturated") },
        Recipe { det: 24.0, boost: 12.0, ctr: 8.0, ..r("R21", "rich clarity") },
        Recipe { ae: 45.0, exp_bright: 10.0, hl: 38.0, sh: 25.0, ..r("R22", "vivid bright tone") },
        Recipe { ae: 38.0, det: 26.0, asat: 20.0, ctr: 18.0, ..r("R23", "punched-up dynamism") },
        Recipe { ae: 52.0, reg: 15, det: 30.0, asat: 26.0, hl: 48.0, sh: 40.0, ..r("R24", "high-energy HDR") },
        Recipe { ae: 45.0, det: 50.0, boost: 32.0, sat: -30.0, ctr: 28.0, ..r("R25", "graphic illustration") },
        Recipe { det: 18.0, boost: 10.0, ..r("R26", "mild detail") },
        Recipe { det: 48.0, boost: 28.0, hl: 16.0, ..r("R27", "heavy detail") },
        Recipe { hl: 60.0, ae: 12.0, ..r("R28", "gentle highlight recovery") },
        Recipe { sh: 62.0, ae: 16.0, ..r("R29", "shadow lift") },
        Recipe { ctr: 23.0, asat: 18.0, vig: 37.0, warm: 8.0, grain: 12.0, ..r("R30", "lo-fi punch and edges") },
        Recipe { ctr: 20.0, sat: 10.0, warm: -8.0, grain: 7.0, tonal_unimplemented: true, ..r("R31", "crossed analog colours") },
        Recipe { ctr: 18.0, sat: -22.0, warm: 18.0, grain: 35.0, vig: 34.0, ..r("R32", "grainy faded vintage") },
        Recipe { warm: -42.0, sat: -4.0, grain: 12.0, ..r("R33", "cool tungsten cast") },
        Recipe { ctr: 46.0, det: 42.0, boost: 26.0, sat: 32.0, tonal_unimplemented: true, ..r("R34", "bold stylised edges") },
        Recipe { ae: 35.0, det: 42.0, boost: 17.0, diff: 24.0, sat: 16.0, ..r("R35", "painterly glow") },
        Recipe { sat: -100.0, ctr: 40.0, det: 48.0, boost: 20.0, vig: 34.0, ..r("R36", "high-contrast monochrome") },
        Recipe { ctr: -12.0, sat: -24.0, warm: 14.0, grain: 13.0, ..r("R37", "washed retro") },
        Recipe { exp_bright: -26.0, ctr: 18.0, sat: -16.0, vig: 28.0, ..r("R38", "moody low key") },
        Recipe { exp_bright: -18.0, sat: -32.0, diff: 25.0, warm: -12.0, ..r("R39", "cold diffuse atmosphere") },
        Recipe { exp_bright: -35.0, ctr: 20.0, sat: -25.0, vig: 38.0, ..r("R40", "dark and dramatic") },
        Recipe { warm: -24.0, sat: -8.0, tonal_unimplemented: true, ..r("R41", "blue-toned grade") },
        Recipe { exp_bright: -18.0, sat: -18.0, vig: 24.0, tonal_unimplemented: true, ..r("R42", "deep shadow grade") },
    ]
}

fn look_for(p: &Recipe) -> Look {
    let mut l = Look::default();
    l.adaptive = AdaptiveExposure {
        strength: p.ae / 100.0,
        regions: p.reg,
        contrast: p.ctr / 100.0,
        brightness_ev: p.exp_bright / 100.0,
        protect_highlights: p.hl / 100.0,
        protect_shadows: p.sh / 100.0,
    };
    l.details = Details {
        strength: 1.0 + p.det / 100.0,
        boost: 1.0 + p.boost / 100.0,
        threshold: 0.08,
        radius: 2.0,
        sharpen: 0.0,
        process_independently: false,
    };
    l.color.adaptive_saturation = p.asat / 100.0;
    l.color.saturation = (1.0 + p.sat / 100.0).max(0.0);
    l.color.boost = (p.sat_boost / 100.0).clamp(-1.0, 1.0);
    l.color.hue_deg = p.hue;
    l.finishing.warmth = p.warm / 100.0;
    l.finishing.vignette_strength = (p.vig / 100.0).max(0.0);
    l.finishing.diffusion = (p.diff / 100.0).max(0.0);
    l.finishing.grain_amount = p.grain / 100.0;
    l
}

fn render(src: &LinearImage, look: &Look) -> LinearImage {
    let mut img = src.clone();
    apply_look(&mut img, look).unwrap();
    img
}

fn luma(p: &[u8]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

/// Std of the fine residual (value minus 9x9 mean) over a centre crop, in code values.
fn texture(rgb: &[u8], w: usize, h: usize) -> f32 {
    let (cw, ch) = (w.min(512), h.min(512));
    let (x0, y0) = ((w - cw) / 2, (h - ch) / 2);
    let y: Vec<f32> = (0..ch)
        .flat_map(|yy| (0..cw).map(move |xx| (yy, xx)))
        .map(|(yy, xx)| luma(&rgb[(((y0 + yy) * w) + x0 + xx) * 3..]))
        .collect();
    let r = 4isize;
    let mut sum = 0.0f64;
    let mut sum2 = 0.0f64;
    let mut n = 0.0f64;
    for yy in 0..ch as isize {
        for xx in 0..cw as isize {
            let mut m = 0.0f32;
            let mut c = 0.0f32;
            for dy in -r..=r {
                for dx in -r..=r {
                    let ay = (yy + dy).clamp(0, ch as isize - 1) as usize;
                    let ax = (xx + dx).clamp(0, cw as isize - 1) as usize;
                    m += y[ay * cw + ax];
                    c += 1.0;
                }
            }
            let v = (y[yy as usize * cw + xx as usize] - m / c) as f64;
            sum += v;
            sum2 += v * v;
            n += 1.0;
        }
    }
    let mean = sum / n;
    ((sum2 / n - mean * mean).max(0.0)).sqrt() as f32
}

fn fractions(rgb: &[u8]) -> (f32, f32, f32) {
    let n = (rgb.len() / 3) as f32;
    let mut hi = 0;
    let mut lo = 0;
    let mut lum = 0.0f64;
    // Luminance-based: a saturated but in-gamut colour has one channel at 255 and must not count.
    for p in rgb.chunks_exact(3) {
        let l = luma(p);
        if l >= 250.0 {
            hi += 1;
        }
        if l <= 5.0 {
            lo += 1;
        }
        lum += l as f64;
    }
    (hi as f32 / n, lo as f32 / n, (lum / n as f64) as f32)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let out_dir = Path::new(&args[1]);
    let photos = &args[2..];
    let list = recipes();
    println!("recipes in table: {}", list.len());
    let mut csv = String::from("photo,recipe,description,texture_ratio,clip_added,crush_added,mean_luma_shift,tonal_unimplemented\n");

    for path in photos {
        let stem = Path::new(path).file_stem().unwrap().to_string_lossy().to_string();
        let src8 = image::open(path).unwrap().to_rgb8();
        let (w0, h0) = src8.dimensions();
        let scale = 1000.0 / w0.max(h0) as f32;
        let (w, h) = ((w0 as f32 * scale.min(1.0)) as u32, (h0 as f32 * scale.min(1.0)) as u32);
        let small = image::imageops::resize(&src8, w, h, FilterType::Triangle);
        let (wu, hu) = (w as usize, h as usize);
        let rgba: Vec<u8> = small.as_raw().chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect();
        let src = LinearImage::from_srgb8(wu, hu, &rgba).unwrap();
        let base = small.as_raw().clone();
        let (hi0, lo0, mean0) = fractions(&base);
        let tex0 = texture(&base, wu, hu);

        let dir = out_dir.join(&stem);
        fs::create_dir_all(&dir).unwrap();
        let thumb_w = 200u32;
        let thumb_h = (h * thumb_w / w).max(1);
        let cols = 6u32;
        let rows = (list.len() as u32 + cols - 1) / cols;
        let mut sheet = RgbImage::from_pixel(cols * (thumb_w + 8), rows * (thumb_h + 8) + 8, image::Rgb([255, 255, 255]));

        for (i, rec) in list.iter().enumerate() {
            let out = render(&src, &look_for(rec));
            let rgb8: Vec<u8> = out.to_srgb8().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
            RgbImage::from_raw(w, h, rgb8.clone()).unwrap().save(dir.join(format!("{}.png", rec.id))).unwrap();
            let (hi, lo, mean) = fractions(&rgb8);
            let tex = texture(&rgb8, wu, hu);
            csv.push_str(&format!(
                "{stem},{},{},{:.3},{:.4},{:.4},{:+.2},{}\n",
                rec.id, rec.what, tex / tex0.max(1e-6), hi - hi0, lo - lo0, mean - mean0, rec.tonal_unimplemented
            ));
            let t = image::imageops::thumbnail(&RgbImage::from_raw(w, h, rgb8).unwrap(), thumb_w, thumb_h);
            let (cx, cy) = ((i as u32 % cols) * (thumb_w + 8), (i as u32 / cols) * (thumb_h + 8) + 8);
            image::imageops::replace(&mut sheet, &t, cx as i64, cy as i64);
        }
        sheet.save(out_dir.join(format!("sheet_{stem}.png"))).unwrap();
        println!("{stem}: input clip {:.4} crush {:.4} texture {:.2}", hi0, lo0, tex0);
    }
    fs::write(out_dir.join("metrics.csv"), csv).unwrap();
    println!("wrote {}", out_dir.join("metrics.csv").display());
}
