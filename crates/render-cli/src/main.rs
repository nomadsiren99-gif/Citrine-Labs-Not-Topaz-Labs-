use std::{env, error::Error, process};

use engine::{apply_exposure, LinearImage};

const USAGE: &str = "usage: render-cli <input> <output> [--ev <stops>]";

fn main() {
    if let Err(err) = run() {
        eprintln!("render-cli: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let (input, output, ev) = parse_args(&args)?;

    // shortcut: 8-bit sRGB only, ICC profiles and 16-bit input are ignored, upgrade in the codec milestone
    let rgba = image::open(input)?.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut img = LinearImage::from_srgb8(w as usize, h as usize, rgba.as_raw())?;
    apply_exposure(&mut img, ev)?;

    let out = image::RgbaImage::from_raw(w, h, img.to_srgb8()).ok_or("output buffer size mismatch")?;
    out.save(output)?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(&str, &str, f32), Box<dyn Error>> {
    let mut positional = Vec::new();
    let mut ev = 0.0f32;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--ev" {
            let raw = args.get(i + 1).ok_or("--ev needs a value")?;
            ev = raw.parse()?;
            i += 2;
        } else {
            positional.push(args[i].as_str());
            i += 1;
        }
    }
    match positional.as_slice() {
        [input, output] => Ok((input, output, ev)),
        _ => Err(USAGE.into()),
    }
}
