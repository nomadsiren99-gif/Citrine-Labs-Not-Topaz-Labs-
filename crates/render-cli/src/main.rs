use std::{collections::HashMap, env, error::Error, process};

use engine::{apply_look, LinearImage, Look};

const USAGE: &str = "usage: render-cli <input> <output> [--ev N] [--adaptive 0..1] [--details 0..2] \
[--saturation 0..2] [--hue DEG] [--denoise 0..1] [--warmth -1..1] [--diffusion 0..1] \
[--vignette 0..1] [--grain 0..1] [--transparency 0..1]";

const FLAGS: [&str; 11] = [
    "--ev", "--adaptive", "--details", "--saturation", "--hue", "--denoise", "--warmth",
    "--diffusion", "--vignette", "--grain", "--transparency",
];

fn main() {
    if let Err(err) = run() {
        eprintln!("render-cli: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let (input, output, values) = parse_args(&args)?;

    // shortcut: 8-bit sRGB only, ICC profiles and 16-bit input are ignored, upgrade in the codec milestone
    let rgba = image::open(input)?.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut img = LinearImage::from_srgb8(w as usize, h as usize, rgba.as_raw())?;

    let get = |k: &str, neutral: f32| values.get(k).copied().unwrap_or(neutral);
    let mut look = Look::default();
    look.ev = get("--ev", 0.0);
    look.adaptive.strength = get("--adaptive", 0.0);
    look.details.strength = get("--details", 1.0);
    look.color.saturation = get("--saturation", 1.0);
    look.color.hue_deg = get("--hue", 0.0);
    look.denoise.amount = get("--denoise", 0.0);
    look.finishing.warmth = get("--warmth", 0.0);
    look.finishing.diffusion = get("--diffusion", 0.0);
    look.finishing.vignette_strength = get("--vignette", 0.0);
    look.finishing.grain_amount = get("--grain", 0.0);
    look.finishing.transparency = get("--transparency", 0.0);
    apply_look(&mut img, &look)?;

    let out = image::RgbaImage::from_raw(w, h, img.to_srgb8()).ok_or("output buffer size mismatch")?;
    out.save(output)?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(&str, &str, HashMap<&str, f32>), Box<dyn Error>> {
    let mut positional = Vec::new();
    let mut values = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(flag) = FLAGS.iter().find(|f| **f == args[i]) {
            let raw = args.get(i + 1).ok_or(format!("{flag} needs a value"))?;
            values.insert(*flag, raw.parse()?);
            i += 2;
        } else if args[i].starts_with("--") {
            return Err(format!("unknown option {}\n{USAGE}", args[i]).into());
        } else {
            positional.push(args[i].as_str());
            i += 1;
        }
    }
    match positional.as_slice() {
        [input, output] => Ok((input, output, values)),
        _ => Err(USAGE.into()),
    }
}
