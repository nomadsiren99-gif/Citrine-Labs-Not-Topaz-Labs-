use std::{collections::HashMap, env, error::Error, process};

use engine::{apply_look, LinearImage, Look};

const USAGE: &str = "usage: render-cli <input> <output> [--ev N] [--adaptive 0..1] [--details 0..2] \
[--saturation 0..2] [--hue DEG] [--denoise 0..1] [--warmth -1..1] [--diffusion 0..1] \
[--vignette 0..1] [--grain 0..1] [--transparency 0..1] [--hdr 0..1] [--hdr-detail 0..2]";

const FLAGS: [&str; 13] = [
    "--ev", "--adaptive", "--details", "--saturation", "--hue", "--denoise", "--warmth",
    "--diffusion", "--vignette", "--grain", "--transparency", "--hdr", "--hdr-detail",
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

    if std::path::Path::new(input).is_dir() {
        return render_dir(input, output, &values);
    }
    render_one(input, output, &values)
}

/// Renders every image in `input_dir` into `output_dir`, keeping file names. The output format
/// follows the file extension of the first output name, so use `out_dir/x.png` style names or pass
/// the output folder with a `.png`, `.tif` or `.jpg` extension sample: `--batch-format` is implied.
fn render_dir(input_dir: &str, output_dir: &str, values: &HashMap<&str, f32>) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(output_dir)?;
    let ext = std::path::Path::new(output_dir).extension().and_then(|e| e.to_str()).unwrap_or("png").to_string();
    let mut count = 0;
    for entry in std::fs::read_dir(input_dir)? {
        let path = entry?.path();
        let is_image = path.extension().and_then(|e| e.to_str()).map(|e| {
            matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "tif" | "tiff")
        });
        if !is_image.unwrap_or(false) {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let out = std::path::Path::new(output_dir).join(format!("{stem}.{ext}"));
        render_one(path.to_str().unwrap(), out.to_str().unwrap(), values)?;
        count += 1;
    }
    eprintln!("render-cli: wrote {count} images to {output_dir}");
    Ok(())
}

fn render_one(input: &str, output: &str, values: &HashMap<&str, f32>) -> Result<(), Box<dyn Error>> {
    // shortcut: ICC profiles are ignored (treated as sRGB), upgrade in the codec milestone.
    // Bit depth follows the input: 16-bit PNG in gives 16-bit PNG out, otherwise 8-bit.
    // EXIF orientation is applied on load, so a photo shot in portrait comes out upright.
    let mut decoder = image::ImageReader::open(input)?.with_guessed_format()?.into_decoder()?;
    let orientation = image::ImageDecoder::orientation(&mut decoder)?;
    let icc = image::ImageDecoder::icc_profile(&mut decoder)?;
    let mut decoded = image::DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    // An embedded profile (for example Display P3 or Adobe RGB) is converted to sRGB, so the
    // engine's sRGB assumption holds. Without a profile the pixels are taken as sRGB.
    if let Some(icc) = icc {
        decoded = to_srgb(&icc, decoded)?;
    }
    let sixteen = matches!(
        decoded.color(),
        image::ColorType::L16 | image::ColorType::La16 | image::ColorType::Rgb16 | image::ColorType::Rgba16
    );
    let (w, h) = (decoded.width(), decoded.height());
    let (wu, hu) = (w as usize, h as usize);
    let mut img = if sixteen {
        LinearImage::from_srgb16(wu, hu, decoded.to_rgba16().as_raw())?
    } else {
        LinearImage::from_srgb8(wu, hu, decoded.to_rgba8().as_raw())?
    };

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
    look.hdr.strength = get("--hdr", 0.0);
    look.hdr.detail = get("--hdr-detail", 1.0);
    // Large images render tile by tile with halo margins, which keeps memory bounded. The tiled
    // result matches the full-frame one (see engine `tiled` tests). Small images go full-frame.
    if w as usize * h as usize > TILE_THRESHOLD_PIXELS {
        img = engine::tiled::apply_look_tiled(&img, &look, TILE_SIZE)?;
    } else {
        apply_look(&mut img, &look)?;
    }

    if sixteen {
        let out = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(w, h, img.to_srgb16())
            .ok_or("output buffer size mismatch")?;
        out.save(output)?;
    } else if is_jpeg(output) {
        // JPEG has no alpha channel: write RGB.
        let rgba = image::RgbaImage::from_raw(w, h, img.to_srgb8()).ok_or("output buffer size mismatch")?;
        image::DynamicImage::ImageRgba8(rgba).to_rgb8().save(output)?;
    } else {
        let out = image::RgbaImage::from_raw(w, h, img.to_srgb8()).ok_or("output buffer size mismatch")?;
        out.save(output)?;
    }
    Ok(())
}

/// Converts pixels from an embedded ICC profile to sRGB. Alpha is carried through unchanged.
fn to_srgb(icc: &[u8], img: image::DynamicImage) -> Result<image::DynamicImage, Box<dyn Error>> {
    use moxcms::{ColorProfile, Layout, TransformOptions};
    let src = ColorProfile::new_from_slice(icc).map_err(|e| format!("bad ICC profile: {e}"))?;
    let dst = ColorProfile::new_srgb();
    let opts = TransformOptions::default();
    let (w, h) = (img.width(), img.height());
    if matches!(img.color(), image::ColorType::L16 | image::ColorType::La16 | image::ColorType::Rgb16 | image::ColorType::Rgba16) {
        let src_px = img.to_rgba16().into_raw();
        let mut out = vec![0u16; src_px.len()];
        let t = src.create_transform_16bit(Layout::Rgba, &dst, Layout::Rgba, opts).map_err(|e| format!("ICC transform: {e}"))?;
        t.transform(&src_px, &mut out).map_err(|e| format!("ICC transform: {e}"))?;
        Ok(image::DynamicImage::ImageRgba16(image::ImageBuffer::from_raw(w, h, out).ok_or("ICC output size")?))
    } else {
        let src_px = img.to_rgba8().into_raw();
        let mut out = vec![0u8; src_px.len()];
        let t = src.create_transform_8bit(Layout::Rgba, &dst, Layout::Rgba, opts).map_err(|e| format!("ICC transform: {e}"))?;
        t.transform(&src_px, &mut out).map_err(|e| format!("ICC transform: {e}"))?;
        Ok(image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, out).ok_or("ICC output size")?))
    }
}

/// Images above this many pixels (about 4 megapixels) use the tiled renderer.
const TILE_THRESHOLD_PIXELS: usize = 4_000_000;
/// Core tile size in pixels for the tiled renderer.
const TILE_SIZE: usize = 1024;

fn is_jpeg(path: &str) -> bool {
    matches!(path.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref(), Some("jpg") | Some("jpeg"))
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
