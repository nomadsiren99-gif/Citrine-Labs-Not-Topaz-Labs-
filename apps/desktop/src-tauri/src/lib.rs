//! Tauri backend. Thin: decodes, scales, calls the engine, encodes. No image maths here.

use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use engine::adaptive_exposure::AdaptiveExposure;
use engine::color_adjust::ColorAdjust;
use engine::curve::Curve;
use engine::details::Details;
use engine::finishing::Finishing;
use engine::local::{Stroke, StrokeKind};
use engine::noise::Denoise;
use engine::quartz_exact::{
    image_buffer::RgbImageF32, params::AdjustParams, pipeline::process_image, preset::get_factory_presets, preset::Preset,
};
use engine::{apply_look, BasicExposure, LinearImage, Look, Style};
use image::{imageops::FilterType, DynamicImage, ImageFormat, RgbaImage};
use serde::Deserialize;

/// Preview longest side in pixels. Proposed, from the spec's 960 px proxy.
const PREVIEW_MAX_SIDE: u32 = 960;

/// One brush dab from the paint tool. Coordinates are fractions of the image.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase", default)]
pub struct Dab {
    pub kind: u8,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub strength: f32,
}

impl Default for Dab {
    fn default() -> Self {
        Self { kind: 0, x: 0.5, y: 0.5, radius: 0.05, hardness: 0.5, opacity: 1.0, strength: 1.0 }
    }
}

fn kind_of(k: u8) -> StrokeKind {
    match k {
        0 => StrokeKind::Dodge,
        1 => StrokeKind::Burn,
        2 => StrokeKind::Smooth,
        3 => StrokeKind::BrushOut,
        _ => StrokeKind::Erase,
    }
}

/// Every control the UI sends. Missing fields take the neutral default.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct Sliders {
    // Basic exposure
    pub ev: f32,
    pub basic_contrast: f32,
    pub boost_blacks: f32,
    pub boost_whites: f32,
    // Adaptive exposure
    pub adaptive: f32,
    pub adaptive_contrast: f32,
    pub adaptive_regions: f32,
    pub adaptive_brightness: f32,
    pub protect_highlights: f32,
    pub protect_shadows: f32,
    // HDR (own method: base compression with detail kept)
    pub hdr_strength: f32,
    pub hdr_detail: f32,
    // Details
    pub details: f32,
    pub detail_boost: f32,
    pub detail_threshold: f32,
    pub detail_radius: f32,
    pub detail_sharpen: f32,
    pub process_independently: bool,
    // Colour
    pub adaptive_saturation: f32,
    pub saturation: f32,
    pub saturation_boost: f32,
    pub hue: f32,
    pub color_regions: f32,
    // Noise
    pub denoise: f32,
    pub noise_suppression: f32,
    // Curve (output level at shadows, mids and highlights)
    pub curve_shadows: f32,
    pub curve_mids: f32,
    pub curve_highlights: f32,
    // Finishing
    pub warmth: f32,
    pub tone_colors: [[f32; 3]; 4],
    pub tone_strength: f32,
    pub diffusion: f32,
    pub vignette: f32,
    pub vignette_size: f32,
    pub vignette_transition: f32,
    pub vignette_center_x: f32,
    pub vignette_center_y: f32,
    pub grain: f32,
    pub grain_size: f32,
    pub grain_seed: u32,
    pub border_size: f32,
    pub border_color: [f32; 3],
    pub transparency: f32,
    // Use the Quartz stage set instead of the default engine
    pub quartz: bool,
    // Paint-tool dabs, applied in order after the single stroke above
    pub brush: Vec<Dab>,
    // One local brush stroke
    pub stroke_enabled: bool,
    pub stroke_kind: u8,
    pub stroke_x: f32,
    pub stroke_y: f32,
    pub stroke_radius: f32,
    pub stroke_hardness: f32,
    pub stroke_opacity: f32,
    pub stroke_strength: f32,
}

impl Default for Sliders {
    fn default() -> Self {
        Self {
            ev: 0.0, basic_contrast: 0.0, boost_blacks: 0.0, boost_whites: 0.0,
            adaptive: 0.0, adaptive_contrast: 0.0, adaptive_regions: 4.0, adaptive_brightness: 0.0,
            protect_highlights: 0.0, protect_shadows: 0.0,
            hdr_strength: 0.0, hdr_detail: 1.0,
            details: 1.0, detail_boost: 1.0, detail_threshold: 0.5, detail_radius: 1.5, detail_sharpen: 0.0,
            process_independently: false,
            adaptive_saturation: 0.0, saturation: 1.0, saturation_boost: 0.0, hue: 0.0, color_regions: 1.0,
            denoise: 0.0, noise_suppression: 1.0,
            curve_shadows: 0.0, curve_mids: 0.5, curve_highlights: 1.0,
            warmth: 0.0, tone_colors: [[1.0; 3]; 4], tone_strength: 0.0, diffusion: 0.0,
            vignette: 0.0, vignette_size: 0.5, vignette_transition: 0.5, vignette_center_x: 0.5, vignette_center_y: 0.5,
            grain: 0.0, grain_size: 1.0, grain_seed: 1, border_size: 0.0, border_color: [0.0; 3], transparency: 0.0,
            quartz: false,
            brush: Vec::new(),
            stroke_enabled: false, stroke_kind: 0, stroke_x: 0.5, stroke_y: 0.5, stroke_radius: 0.2,
            stroke_hardness: 0.5, stroke_opacity: 1.0, stroke_strength: 1.0,
        }
    }
}

impl Sliders {
    fn look(self) -> Look {
        let curve_default = (self.curve_shadows, self.curve_mids, self.curve_highlights) == (0.0, 0.5, 1.0);
        let stroke_kind = kind_of(self.stroke_kind);
        Look {
            style: if self.quartz { Style::Quartz } else { Style::Ours },
            ev: self.ev,
            basic: BasicExposure { contrast: self.basic_contrast, boost_blacks: self.boost_blacks, boost_whites: self.boost_whites },
            adaptive: AdaptiveExposure {
                strength: self.adaptive,
                regions: self.adaptive_regions.round().max(1.0) as u32,
                contrast: self.adaptive_contrast,
                brightness_ev: self.adaptive_brightness,
                protect_highlights: self.protect_highlights,
                protect_shadows: self.protect_shadows,
            },
            hdr: engine::hdr::Hdr { strength: self.hdr_strength, detail: self.hdr_detail },
            details: Details {
                strength: self.details,
                boost: self.detail_boost,
                threshold: self.detail_threshold,
                radius: self.detail_radius,
                sharpen: self.detail_sharpen,
                process_independently: self.process_independently,
            },
            color: ColorAdjust {
                adaptive_saturation: self.adaptive_saturation,
                saturation: self.saturation,
                boost: self.saturation_boost,
                hue_deg: self.hue,
                color_regions: self.color_regions,
            },
            denoise: Denoise { amount: self.denoise, suppression: self.noise_suppression },
            curve: if curve_default {
                Curve::default()
            } else {
                Curve { points: vec![(0.0, self.curve_shadows), (0.5, self.curve_mids), (1.0, self.curve_highlights)] }
            },
            strokes: self
                .stroke_enabled
                .then(|| Stroke {
                    kind: stroke_kind,
                    center: [self.stroke_x, self.stroke_y],
                    radius: self.stroke_radius,
                    hardness: self.stroke_hardness,
                    opacity: self.stroke_opacity,
                    overall_strength: self.stroke_strength,
                    edge_aware: 0.0,
                })
                .into_iter()
                .chain(self.brush.iter().map(|d| Stroke {
                    kind: kind_of(d.kind),
                    center: [d.x, d.y],
                    radius: d.radius,
                    hardness: d.hardness,
                    opacity: d.opacity,
                    overall_strength: d.strength,
                    edge_aware: 0.0,
                }))
                .collect(),
            finishing: Finishing {
                warmth: self.warmth,
                tone_colors: self.tone_colors,
                tone_strength: self.tone_strength,
                diffusion: self.diffusion,
                vignette_strength: self.vignette,
                vignette_size: self.vignette_size,
                vignette_transition: self.vignette_transition,
                vignette_center: [self.vignette_center_x, self.vignette_center_y],
                grain_amount: self.grain,
                grain_size: self.grain_size,
                grain_seed: self.grain_seed,
                border_size: self.border_size,
                border_color: self.border_color,
                transparency: self.transparency,
                ..Default::default()
            },
        }
    }
}

/// Hand-off format shared with the Photoshop plug-in: magic "CTRN", width and height as u32
/// little-endian, then tightly packed RGB8 rows. Raw on purpose: the plug-in writes it without codecs.
const RAW_MAGIC: &[u8; 4] = b"CTRN";

fn read_raw(path: &str) -> Result<RgbaImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read file: {e}"))?;
    if bytes.len() < 12 || &bytes[..4] != RAW_MAGIC {
        return Err("not a Citrine hand-off file".into());
    }
    let w = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let h = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let body = &bytes[12..];
    if w == 0 || h == 0 || body.len() != (w as usize) * (h as usize) * 3 {
        return Err("hand-off file size does not match its header".into());
    }
    let rgba: Vec<u8> = body.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    RgbaImage::from_raw(w, h, rgba).ok_or_else(|| "bad hand-off dimensions".to_string())
}

fn load(path: &str) -> Result<RgbaImage, String> {
    if path.to_ascii_lowercase().ends_with(".ctrn") {
        return read_raw(path);
    }
    // EXIF orientation is applied on load, matching the CLI.
    let err = |e: image::ImageError| format!("cannot open image: {e}");
    let mut decoder = image::ImageReader::open(path).map_err(|e| format!("cannot open image: {e}"))?
        .with_guessed_format().map_err(|e| format!("cannot open image: {e}"))?
        .into_decoder().map_err(err)?;
    let orientation = image::ImageDecoder::orientation(&mut decoder).map_err(err)?;
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(err)?;
    img.apply_orientation(orientation);
    Ok(img.to_rgba8())
}

fn render(src: &RgbaImage, look: &Look) -> Result<RgbaImage, String> {
    let (w, h) = src.dimensions();
    let mut img = LinearImage::from_srgb8(w as usize, h as usize, src.as_raw()).map_err(|e| e.to_string())?;
    // Large images render tile by tile with halo margins (same result, bounded memory).
    if (w as usize) * (h as usize) > 4_000_000 {
        img = engine::tiled::apply_look_tiled(&img, look, 1024).map_err(|e| e.to_string())?;
    } else {
        apply_look(&mut img, look).map_err(|e| e.to_string())?;
    }
    RgbaImage::from_raw(w, h, img.to_srgb8()).ok_or_else(|| "output buffer size mismatch".to_string())
}

/// Renders a downscaled preview and returns it as a base64 PNG.
#[tauri::command]
fn render_preview(path: String, sliders: Sliders) -> Result<String, String> {
    let full = load(&path)?;
    let scale = (PREVIEW_MAX_SIDE as f32 / full.width().max(full.height()) as f32).min(1.0);
    let small = if scale < 1.0 {
        let (w, h) = ((full.width() as f32 * scale).round() as u32, (full.height() as f32 * scale).round() as u32);
        image::imageops::resize(&full, w.max(1), h.max(1), FilterType::Triangle)
    } else {
        full
    };
    let out = render(&small, &sliders.look())?;
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(out).write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(STANDARD.encode(buf.into_inner()))
}

/// Renders at full resolution and writes the file. Format comes from the output extension.
/// Zoom: a full-resolution crop at 1:1, rendered with the same whole-image values as the full
/// render. The source image and the measured plan are cached, so panning is quick until a slider moves.
struct ZoomCache {
    path: String,
    look_key: String,
    src: LinearImage,
    plan: engine::tiled::Plan,
}

static ZOOM: std::sync::Mutex<Option<ZoomCache>> = std::sync::Mutex::new(None);

/// Width and height of the image at `path`, in pixels.
#[tauri::command]
fn image_size(path: String) -> Result<(u32, u32), String> {
    let img = load(&path)?;
    Ok((img.width(), img.height()))
}

/// Renders the rectangle `(x, y, w, h)` of the image at full resolution, as a base64 PNG.
#[tauri::command]
fn render_crop(path: String, sliders: Sliders, x: u32, y: u32, w: u32, h: u32) -> Result<String, String> {
    let look = sliders.look();
    let look_key = format!("{look:?}");
    let mut cache = ZOOM.lock().map_err(|e| e.to_string())?;
    let fresh = match cache.as_ref() {
        Some(c) => c.path != path || c.look_key != look_key,
        None => true,
    };
    if fresh {
        let src_rgba = load(&path)?;
        let (sw, sh) = src_rgba.dimensions();
        let src = LinearImage::from_srgb8(sw as usize, sh as usize, src_rgba.as_raw()).map_err(|e| e.to_string())?;
        let plan = engine::tiled::Plan::new(&src, &look, 1024).map_err(|e| e.to_string())?;
        *cache = Some(ZoomCache { path: path.clone(), look_key, src, plan });
    }
    let c = cache.as_ref().expect("cache filled above");
    let (sw, sh) = (c.src.width() as u32, c.src.height() as u32);
    let x = x.min(sw.saturating_sub(1));
    let y = y.min(sh.saturating_sub(1));
    let w = w.min(sw - x).max(1);
    let h = h.min(sh - y).max(1);
    let out = c.plan.render_region(&c.src, (x as usize, y as usize, w as usize, h as usize)).map_err(|e| e.to_string())?;
    let rgba = RgbaImage::from_raw(w, h, out.to_srgb8()).ok_or("crop buffer size mismatch")?;
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(rgba).write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(STANDARD.encode(buf.into_inner()))
}

#[tauri::command]
fn export_image(path: String, out_path: String, sliders: Sliders) -> Result<(), String> {
    let full = load(&path)?;
    let out = render(&full, &sliders.look())?;
    out.save(&out_path).map_err(|e| format!("cannot write image: {e}"))
}

/// Quartz's exact factory presets (names and parameters as in the author's repo).
#[tauri::command]
fn quartz_presets() -> Vec<Preset> {
    get_factory_presets()
}

/// Runs Quartz's exact pipeline on RGB. Alpha is kept from the source image.
fn quartz_render(src: &RgbaImage, params: &AdjustParams) -> RgbaImage {
    let rgb = image::DynamicImage::ImageRgba8(src.clone()).to_rgb8();
    let input = RgbImageF32::from_rgb8(&rgb);
    let out = process_image(&input, params).to_rgb8();
    let mut result = RgbaImage::new(src.width(), src.height());
    for (x, y, px) in result.enumerate_pixels_mut() {
        let c = out.get_pixel(x, y).0;
        *px = image::Rgba([c[0], c[1], c[2], src.get_pixel(x, y).0[3]]);
    }
    result
}

/// The preview source (decoded and downscaled to PREVIEW_MAX_SIDE) for the last image opened. Slider
/// changes reuse it, so the photo is not decoded again on every change.
static PREVIEW_SOURCE: std::sync::Mutex<Option<(String, RgbaImage)>> = std::sync::Mutex::new(None);

fn preview_source(path: &str, side: u32) -> Result<RgbaImage, String> {
    let mut cache = PREVIEW_SOURCE.lock().map_err(|e| e.to_string())?;
    if cache.as_ref().map(|(p, _)| p != path).unwrap_or(true) {
        let full = load(path)?;
        let scale = (PREVIEW_MAX_SIDE as f32 / full.width().max(full.height()) as f32).min(1.0);
        let small = if scale < 1.0 {
            let (w, h) = ((full.width() as f32 * scale).round() as u32, (full.height() as f32 * scale).round() as u32);
            image::imageops::resize(&full, w.max(1), h.max(1), FilterType::Triangle)
        } else {
            full
        };
        *cache = Some((path.to_string(), small));
    }
    let (_, src) = cache.as_ref().expect("cache filled above");
    let longest = src.width().max(src.height());
    if side >= longest {
        return Ok(src.clone());
    }
    let f = side as f32 / longest as f32;
    let (w, h) = ((src.width() as f32 * f).round().max(1.0) as u32, (src.height() as f32 * f).round().max(1.0) as u32);
    Ok(image::imageops::resize(src, w, h, FilterType::Triangle))
}

/// Exact Quartz preview. `side` is the longest side in pixels: a small draft while a slider moves,
/// the full preview size once it settles. Defaults to PREVIEW_MAX_SIDE.
#[tauri::command]
fn render_quartz_preview(path: String, params: AdjustParams, side: Option<u32>) -> Result<String, String> {
    let small = preview_source(&path, side.unwrap_or(PREVIEW_MAX_SIDE))?;
    let out = quartz_render(&small, &params);
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(out).write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(STANDARD.encode(buf.into_inner()))
}

/// Full-resolution Quartz output for the zoom views, kept for the last image and settings. Panning
/// crops from it, so only a settings change renders the whole picture again.
static ZOOM_QUARTZ: std::sync::Mutex<Option<(String, String, RgbaImage)>> = std::sync::Mutex::new(None);

/// Full-resolution Quartz crop `(x, y, w, h)` for the zoom views, as a base64 PNG.
#[tauri::command]
fn render_quartz_crop(path: String, params: AdjustParams, x: u32, y: u32, w: u32, h: u32) -> Result<String, String> {
    let key = serde_json::to_string(&params).map_err(|e| e.to_string())?;
    let mut cache = ZOOM_QUARTZ.lock().map_err(|e| e.to_string())?;
    let fresh = cache.as_ref().map(|(p, k, _)| p != &path || k != &key).unwrap_or(true);
    if fresh {
        let full = load(&path)?;
        *cache = Some((path.clone(), key, quartz_render(&full, &params)));
    }
    let (_, _, out) = cache.as_ref().expect("cache filled above");
    let x = x.min(out.width().saturating_sub(1));
    let y = y.min(out.height().saturating_sub(1));
    let w = w.min(out.width() - x).max(1);
    let h = h.min(out.height() - y).max(1);
    let crop = image::imageops::crop_imm(out, x, y, w, h).to_image();
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(crop).write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(STANDARD.encode(buf.into_inner()))
}

/// Exact Quartz export at full resolution.
#[tauri::command]
fn export_quartz(path: String, out_path: String, params: AdjustParams) -> Result<(), String> {
    let full = load(&path)?;
    quartz_render(&full, &params).save(&out_path).map_err(|e| format!("cannot write image: {e}"))
}

/// Writes a preset (JSON text) to a path the user chose in the save dialog.
#[tauri::command]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents).map_err(|e| format!("cannot write file: {e}"))
}

/// Reads a preset (JSON text) from a path the user chose in the open dialog.
#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("cannot read file: {e}"))
}

/// File opened from Finder or Photoshop before the window listens for it. The window asks for it
/// on start, since an open event at launch can fire before the page is ready.
use tauri::Manager;

#[derive(Default)]
struct PendingOpen(std::sync::Mutex<Option<String>>);

/// Returns the file opened at launch, once.
#[tauri::command]
fn take_pending_open(state: tauri::State<PendingOpen>) -> Option<String> {
    state.0.lock().ok()?.take()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(PendingOpen::default())
        .invoke_handler(tauri::generate_handler![
            render_preview, export_image, write_text_file, read_text_file, image_size, render_crop,
            quartz_presets, render_quartz_preview, render_quartz_crop, export_quartz, take_pending_open
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // A file opened from Finder or Photoshop: tell the window to load it.
            if let tauri::RunEvent::Opened { urls } = event {
                for url in urls {
                    if let Ok(p) = url.to_file_path() {
                        let path = p.to_string_lossy().to_string();
                        if let Ok(mut pending) = app.state::<PendingOpen>().0.lock() {
                            *pending = Some(path.clone());
                        }
                        let _ = tauri::Emitter::emit(app, "open-file", path);
                    }
                }
            }
        });
}

#[cfg(test)]
mod handoff_tests {
    use super::*;

    fn write_raw(path: &str, img: &RgbaImage) -> Result<(), String> {
        let mut out = Vec::with_capacity(12 + (img.width() * img.height() * 3) as usize);
        out.extend_from_slice(RAW_MAGIC);
        out.extend_from_slice(&img.width().to_le_bytes());
        out.extend_from_slice(&img.height().to_le_bytes());
        out.extend(img.pixels().flat_map(|p| [p[0], p[1], p[2]]));
        std::fs::write(path, out).map_err(|e| format!("cannot write file: {e}"))
    }

    #[test]
    fn raw_handoff_round_trips_rgb() {
        let img = RgbaImage::from_fn(7, 5, |x, y| image::Rgba([(x * 30) as u8, (y * 40) as u8, 9, 255]));
        let path = std::env::temp_dir().join(format!("citrine_handoff_{}.ctrn", std::process::id()));
        let p = path.to_str().unwrap();
        write_raw(p, &img).unwrap();
        let back = load(p).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back, img);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_png(name: &str, w: u32, h: u32) -> String {
        let path = std::env::temp_dir().join(format!("photo-editor-test-{name}.png"));
        let img = RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255]));
        img.save(&path).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn neutral_preview_is_scaled_to_the_preview_size() {
        let src = temp_png("neutral", 1200, 600);
        let b64 = render_preview(src.clone(), Sliders::default()).unwrap();
        let decoded = image::load_from_memory(&STANDARD.decode(b64).unwrap()).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (960, 480));
        let _ = std::fs::remove_file(src);
    }

    #[test]
    fn export_at_full_size_is_identical_when_neutral() {
        let src = temp_png("export", 64, 32);
        let out = std::env::temp_dir().join("photo-editor-test-export.png");
        export_image(src.clone(), out.to_string_lossy().into_owned(), Sliders::default()).unwrap();
        let a = image::open(&src).unwrap().to_rgba8();
        let b = image::open(&out).unwrap().to_rgba8();
        assert_eq!(a.dimensions(), b.dimensions());
        let max_diff = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| (*x as i32 - *y as i32).abs()).max().unwrap();
        assert!(max_diff <= 0, "neutral export changed pixels by {max_diff}");
        let _ = std::fs::remove_file(src);
        let _ = std::fs::remove_file(out);
    }

    #[test]
    fn missing_file_reports_an_error() {
        assert!(render_preview("/nonexistent/x.png".into(), Sliders::default()).is_err());
    }

    #[test]
    fn brush_dabs_add_strokes_after_the_single_stroke() {
        let s = Sliders {
            stroke_enabled: true,
            brush: vec![Dab::default(), Dab { kind: 1, ..Dab::default() }],
            ..Sliders::default()
        };
        let look = s.look();
        assert_eq!(look.strokes.len(), 3);
        assert_eq!(look.strokes[2].kind, StrokeKind::Burn);
    }

    #[test]
    fn partial_json_takes_neutral_defaults() {
        let s: Sliders = serde_json::from_str(r#"{"warmth": 0.4}"#).unwrap();
        assert_eq!(s.warmth, 0.4);
        assert_eq!(s.saturation, 1.0);
    }

    #[test]
    fn quartz_neutral_is_identical() {
        let src = RgbaImage::from_fn(40, 20, |x, y| image::Rgba([(x * 6) as u8, (y * 12) as u8, 90, 200]));
        let out = quartz_render(&src, &AdjustParams::default());
        assert_eq!(out, src, "neutral Quartz render must not change pixels or alpha");
    }

    #[test]
    fn every_quartz_factory_preset_renders() {
        let src = RgbaImage::from_fn(64, 48, |x, y| image::Rgba([(x * 3) as u8, (y * 5) as u8, 120, 255]));
        let presets = quartz_presets();
        assert_eq!(presets.len(), 157);
        for p in presets {
            let out = quartz_render(&src, &p.params);
            assert_eq!(out.dimensions(), (64, 48), "preset {}", p.name);
            assert!(out.pixels().all(|px| px.0[3] == 255), "alpha changed in {}", p.name);
        }
    }
}
