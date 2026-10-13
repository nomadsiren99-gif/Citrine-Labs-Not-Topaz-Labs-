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
    image_buffer::RgbImageF32, params::{AdjustParams, BrushDab}, pipeline::{process_image, process_region}, preset::get_factory_presets, preset::Preset,
};
use engine::{apply_look, BasicExposure, LinearImage, Look, Style};
use image::{imageops::FilterType, DynamicImage, ImageFormat, RgbaImage};
use serde::Deserialize;

/// Preview longest side in pixels. Proposed, from the spec's 960 px proxy.
/// Longest side of a preset thumbnail in the library.
const PRESET_THUMB_SIDE: u32 = 160;
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
    if bytes.len() >= 12 && &bytes[..4] == RAW16_MAGIC {
        let (w, h, px) = parse_raw16(&bytes)?;
        let rgba: Vec<u8> = px.chunks_exact(3).flat_map(|p| [(p[0] / 257) as u8, (p[1] / 257) as u8, (p[2] / 257) as u8, 255]).collect();
        return RgbaImage::from_raw(w, h, rgba).ok_or_else(|| "bad hand-off dimensions".to_string());
    }
    if bytes.len() < 12 || &bytes[..4] != RAW_MAGIC {
        return Err("not a Citrine hand-off file".into());
    }
    let w = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let h = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    check_size(w as usize, h as usize)?;
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
    let (dw, dh) = image::ImageDecoder::dimensions(&decoder);
    check_size(dw as usize, dh as usize)?;
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

/// Runs heavy image work on a blocking thread. Plain Tauri commands run on the main thread, and a
/// full-resolution render there freezes the window.
async fn off_thread<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// Renders a downscaled preview and returns it as a base64 PNG.
#[tauri::command]
async fn render_preview(path: String, sliders: Sliders) -> Result<String, String> {
    off_thread(move || -> Result<_, String> {
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
    }).await
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
async fn image_size(path: String) -> Result<(u32, u32, bool), String> {
    off_thread(move || -> Result<_, String> {
    let (img, deep) = decode_full(&path)?;
    Ok((img.width as u32, img.height as u32, deep))
    }).await
}

/// Renders the rectangle `(x, y, w, h)` of the image at full resolution, as a base64 PNG.
#[tauri::command]
async fn render_crop(path: String, sliders: Sliders, x: u32, y: u32, w: u32, h: u32) -> Result<String, String> {
    off_thread(move || -> Result<_, String> {
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
    }).await
}

#[tauri::command]
async fn export_image(path: String, out_path: String, sliders: Sliders) -> Result<(), String> {
    off_thread(move || -> Result<_, String> {
    let full = load(&path)?;
    let out = render(&full, &sliders.look())?;
    out.save(&out_path).map_err(|e| format!("cannot write image: {e}"))
    }).await
}

/// Quartz's exact factory presets (names and parameters as in the author's repo).
#[tauri::command]
fn quartz_presets() -> Vec<Preset> {
    get_factory_presets()
}

/// Brush dabs as engine strokes. Dabs with a non-finite value are dropped rather than failing the render.
fn brush_strokes(brush: &[BrushDab]) -> Vec<Stroke> {
    brush
        .iter()
        .filter(|d| [d.x, d.y, d.radius, d.hardness, d.opacity, d.strength].iter().all(|v| v.is_finite()))
        .map(|d| Stroke {
            kind: kind_of(d.kind),
            center: [d.x, d.y],
            radius: d.radius,
            hardness: d.hardness,
            opacity: d.opacity,
            overall_strength: d.strength,
            edge_aware: 0.0,
        })
        .collect()
}

/// Applies the brush dabs to an 8-bit RGBA image (preview and zoom). Alpha is kept.
fn apply_brush_rgba(img: &mut RgbaImage, brush: &[BrushDab]) -> Result<(), String> {
    let strokes = brush_strokes(brush);
    if strokes.is_empty() {
        return Ok(());
    }
    let (w, h) = img.dimensions();
    let mut lin = LinearImage::from_srgb8(w as usize, h as usize, img.as_raw()).map_err(|e| e.to_string())?;
    let original = lin.clone();
    engine::local::apply(&mut lin, &original, &strokes).map_err(|e| e.to_string())?;
    let out = lin.to_srgb8();
    for (px, c) in img.pixels_mut().zip(out.chunks_exact(4)) {
        px.0 = [c[0], c[1], c[2], px.0[3]];
    }
    Ok(())
}

/// Applies the brush dabs to a float RGB image at full precision (export). Goes through 16-bit sRGB,
/// which is finer than any source file the app writes back.
fn apply_brush_rgb(img: &mut RgbImageF32, brush: &[BrushDab]) -> Result<(), String> {
    let strokes = brush_strokes(brush);
    if strokes.is_empty() {
        return Ok(());
    }
    let q = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let mut buf = Vec::with_capacity(img.r.len() * 4);
    for i in 0..img.r.len() {
        buf.extend_from_slice(&[q(img.r[i]), q(img.g[i]), q(img.b[i]), 65535]);
    }
    let mut lin = LinearImage::from_srgb16(img.width, img.height, &buf).map_err(|e| e.to_string())?;
    let original = lin.clone();
    engine::local::apply(&mut lin, &original, &strokes).map_err(|e| e.to_string())?;
    let out = lin.to_srgb16();
    for i in 0..img.r.len() {
        img.r[i] = out[i * 4] as f32 / 65535.0;
        img.g[i] = out[i * 4 + 1] as f32 / 65535.0;
        img.b[i] = out[i * 4 + 2] as f32 / 65535.0;
    }
    Ok(())
}








/// Quartz's exact pipeline on RGB, without the brush or the dodge and burn layers. Alpha is kept.
/// This is the slow part, so callers cache it and only re-apply the layers when those change.
fn quartz_base(src: &RgbaImage, params: &AdjustParams) -> RgbaImage {
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

/// Brush dabs, then dodge and burn layers, on a base render. Fast: masks and gains only.
fn finish_layers(img: &mut RgbaImage, params: &AdjustParams) {
    // A brush error leaves the image without dabs rather than failing the preview.
    let _ = apply_brush_rgba(img, &params.brush);
}

/// Cache key for a base render: every setting except the brush and the layers, which are applied later.
fn base_key(params: &AdjustParams) -> String {
    let mut p = params.clone();
    p.brush.clear();
    serde_json::to_string(&p).unwrap_or_default()
}

/// The base render for `params` (without layers) on `src`, reused while the slider settings stay the same.
fn cached_base(cache: &std::sync::Mutex<Option<(String, RgbaImage)>>, key_prefix: &str, src: &RgbaImage, params: &AdjustParams) -> RgbaImage {
    let key = format!("{key_prefix}|{}", base_key(params));
    let mut slot = match cache.lock() {
        Ok(s) => s,
        Err(_) => return quartz_base(src, params),
    };
    if let Some((k, img)) = slot.as_ref() {
        if *k == key {
            return img.clone();
        }
    }
    let img = quartz_base(src, params);
    *slot = Some((key, img.clone()));
    img
}

/// Quartz render with the brush and layers applied (used for the full image in export-like paths).
fn quartz_render(src: &RgbaImage, params: &AdjustParams) -> RgbaImage {
    let mut result = quartz_base(src, params);
    finish_layers(&mut result, params);
    result
}

/// Cached base render for the preview (per path and size), and for the spatial GPU base.
static PREVIEW_BASE: std::sync::Mutex<Option<(String, RgbaImage)>> = std::sync::Mutex::new(None);
static SPATIAL_BASE: std::sync::Mutex<Option<(String, RgbaImage)>> = std::sync::Mutex::new(None);

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
async fn render_quartz_preview(path: String, params: AdjustParams, side: Option<u32>) -> Result<String, String> {
    off_thread(move || -> Result<_, String> {
    let small = preview_source(&path, side.unwrap_or(PREVIEW_MAX_SIDE))?;
    let mut out = cached_base(&PREVIEW_BASE, &format!("{path}|{}", small.width()), &small, &params);
    finish_layers(&mut out, &params);
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(out).write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(STANDARD.encode(buf.into_inner()))
    }).await
}

/// Full-resolution crop kept for the zoom views, for the last image and settings key.
static ZOOM_QUARTZ: std::sync::Mutex<Option<(String, String, RgbaImage)>> = std::sync::Mutex::new(None);

/// Full-resolution spatial base (colour, tone and curve left to the GPU), kept per image and spatial key.
static ZOOM_BASE: std::sync::Mutex<Option<(String, String, RgbaImage)>> = std::sync::Mutex::new(None);

/// Raw RGBA behind a width and height header (u32, little-endian): no PNG encode or decode.
fn raw_response(img: &RgbaImage) -> tauri::ipc::Response {
    let mut bytes = Vec::with_capacity(8 + img.as_raw().len());
    bytes.extend_from_slice(&img.width().to_le_bytes());
    bytes.extend_from_slice(&img.height().to_le_bytes());
    bytes.extend_from_slice(img.as_raw());
    tauri::ipc::Response::new(bytes)
}

/// Crops `(x, y, w, h)` from the full-resolution render of `render` (rendered once per `key`), as raw RGBA.
fn crop_cached(
    cache: &std::sync::Mutex<Option<(String, String, RgbaImage)>>,
    path: &str,
    render: &AdjustParams,
    finish: &AdjustParams,
    (x, y, w, h): (u32, u32, u32, u32),
) -> Result<tauri::ipc::Response, String> {
    let key_json = base_key(render);
    let mut cache = cache.lock().map_err(|e| e.to_string())?;
    let fresh = cache.as_ref().map(|(p, k, _)| p != path || *k != key_json).unwrap_or(true);
    if fresh {
        let full = load(path)?;
        *cache = Some((path.to_string(), key_json, quartz_base(&full, render)));
    }
    let (_, _, base) = cache.as_ref().expect("cache filled above");
    let mut out = base.clone();
    finish_layers(&mut out, finish);
    let out = &out;
    let x = x.min(out.width().saturating_sub(1));
    let y = y.min(out.height().saturating_sub(1));
    let w = w.min(out.width() - x).max(1);
    let h = h.min(out.height() - y).max(1);
    Ok(raw_response(&image::imageops::crop_imm(out, x, y, w, h).to_image()))
}

/// Full-resolution Quartz crop `(x, y, w, h)` for the zoom views, as raw RGBA.
#[tauri::command]
async fn render_quartz_crop(path: String, params: AdjustParams, x: u32, y: u32, w: u32, h: u32) -> Result<tauri::ipc::Response, String> {
    off_thread(move || -> Result<_, String> {
    crop_cached(&ZOOM_QUARTZ, &path, &params, &params, (x, y, w, h))
    }).await
}

/// Parameters with the per-pixel stages off (colour, tone, curve, finishing). The GPU applies those, so
/// the CPU renders only the stages that depend on the rest of the image.
fn spatial_only(params: &AdjustParams) -> AdjustParams {
    let mut p = params.clone();
    p.color.enabled = false;
    p.tone.enabled = false;
    p.curve.enabled = false;
    p.finishing.enabled = false;
    p.basic_correction.enabled = false;
    p.grading.enabled = false;
    p
}

/// Spatial base for the GPU preview at preview size, as raw RGBA.
#[tauri::command]
async fn render_base_preview(path: String, params: AdjustParams, side: Option<u32>) -> Result<tauri::ipc::Response, String> {
    off_thread(move || -> Result<_, String> {
    let small = preview_source(&path, side.unwrap_or(PREVIEW_MAX_SIDE))?;
    let mut out = cached_base(&SPATIAL_BASE, &format!("{path}|{}", small.width()), &small, &spatial_only(&params));
    finish_layers(&mut out, &params);
    Ok(raw_response(&out))
    }).await
}

/// Full-resolution spatial base crop for the GPU zoom view, as raw RGBA.
#[tauri::command]
async fn render_base_crop(path: String, params: AdjustParams, x: u32, y: u32, w: u32, h: u32) -> Result<tauri::ipc::Response, String> {
    off_thread(move || -> Result<_, String> {
    crop_cached(&ZOOM_BASE, &path, &spatial_only(&params), &params, (x, y, w, h))
    }).await
}

/// Red, green and blue curve tables (1024 entries each), composed as Quartz's `apply_curve` does,
/// as f32 little-endian. Sampled on the GPU with the same index rule.
#[tauri::command]
async fn curve_luts(params: AdjustParams) -> Result<tauri::ipc::Response, String> {
    off_thread(move || -> Result<_, String> {
    use engine::quartz_exact::stages::curve::generate_curve_lut;
    let c = &params.curve;
    let active = |pts: &[engine::quartz_exact::params::CurvePoint]| pts.len() >= 2;
    let master = generate_curve_lut(&c.points);
    let sample = |lut: &[f32; 1024], v: f32| lut[((v.clamp(0.0, 1.0) * 1023.0).round() as usize).min(1023)];
    let compose = |channel: &[engine::quartz_exact::params::CurvePoint]| -> [f32; 1024] {
        if !active(channel) {
            return master;
        }
        let ch = generate_curve_lut(channel);
        let mut out = [0.0f32; 1024];
        for (o, &m) in out.iter_mut().zip(master.iter()) {
            *o = sample(&ch, m);
        }
        out
    };
    let mut bytes = Vec::with_capacity(3 * 1024 * 4);
    for lut in [compose(&c.red), compose(&c.green), compose(&c.blue)] {
        for v in lut {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    Ok(tauri::ipc::Response::new(bytes))
    }).await
}

/// A preset applied to the open image at thumbnail size, as a base64 PNG for the preset library.
#[tauri::command]
async fn preset_thumbnail(path: String, params: AdjustParams) -> Result<String, String> {
    off_thread(move || -> Result<_, String> {
        let small = preview_source(&path, PRESET_THUMB_SIDE)?;
        let out = quartz_render(&small, &params);
        let mut buf = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(out).write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(STANDARD.encode(buf.into_inner()))
    })
    .await
}

/// The 16-bit hand-off ("CTR6"): width and height as u32 little-endian, then RGB as u16 (0..65535).
const RAW16_MAGIC: &[u8; 4] = b"CTR6";

/// Parses a 16-bit hand-off into width, height and the samples (0..65535).
fn parse_raw16(bytes: &[u8]) -> Result<(u32, u32, Vec<u16>), String> {
    if bytes.len() < 12 || &bytes[..4] != RAW16_MAGIC {
        return Err("not a 16-bit Citrine hand-off file".into());
    }
    let w = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let h = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let body = &bytes[12..];
    if w == 0 || h == 0 || body.len() != (w as usize) * (h as usize) * 6 {
        return Err("hand-off file size does not match its header".into());
    }
    let px = body.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    Ok((w, h, px))
}

/// Decodes an image at full precision: RGB in 0..1, and whether the file stores more than 8 bits per
/// channel. 16-bit PNG and TIFF keep every level; 8-bit files are exact on the same scale.
fn decode_full(path: &str) -> Result<(RgbImageF32, bool), String> {
    if path.to_ascii_lowercase().ends_with(".ctrn") {
        let bytes = std::fs::read(path).map_err(|e| format!("cannot read file: {e}"))?;
        if bytes.len() >= 4 && &bytes[..4] == RAW16_MAGIC {
            let (w, h, px) = parse_raw16(&bytes)?;
            let mut out = RgbImageF32::new(w as usize, h as usize);
            for (i, p) in px.chunks_exact(3).enumerate() {
                out.r[i] = p[0] as f32 / 65535.0;
                out.g[i] = p[1] as f32 / 65535.0;
                out.b[i] = p[2] as f32 / 65535.0;
            }
            return Ok((out, true));
        }
        let rgb = image::DynamicImage::ImageRgba8(read_raw(path)?).to_rgb8();
        return Ok((RgbImageF32::from_rgb8(&rgb), false));
    }
    let err = |e: image::ImageError| format!("cannot open image: {e}");
    let mut decoder = image::ImageReader::open(path).map_err(|e| format!("cannot open image: {e}"))?
        .with_guessed_format().map_err(|e| format!("cannot open image: {e}"))?
        .into_decoder().map_err(err)?;
    let orientation = image::ImageDecoder::orientation(&mut decoder).map_err(err)?;
    let (dw, dh) = image::ImageDecoder::dimensions(&decoder);
    check_size(dw as usize, dh as usize)?;
    let ct = image::ImageDecoder::color_type(&decoder);
    let deep = ct.bits_per_pixel() / u16::from(ct.channel_count()) > 8;
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(err)?;
    img.apply_orientation(orientation);
    let rgb16 = img.into_rgb16();
    let (w, h) = (rgb16.width() as usize, rgb16.height() as usize);
    let mut out = RgbImageF32::new(w, h);
    for (i, px) in rgb16.pixels().enumerate() {
        out.r[i] = px[0] as f32 / 65535.0;
        out.g[i] = px[1] as f32 / 65535.0;
        out.b[i] = px[2] as f32 / 65535.0;
    }
    Ok((out, deep))
}

/// Writes a processed image. A 16-bit source gives a 16-bit PNG or TIFF; anything else is 8-bit.
fn write_export(out: &RgbImageF32, out_path: &str, deep: bool) -> Result<(), String> {
    let lower = out_path.to_ascii_lowercase();
    let can_deep = lower.ends_with(".png") || lower.ends_with(".tif") || lower.ends_with(".tiff");
    if deep && can_deep {
        let (w, h) = (out.width as u32, out.height as u32);
        let q = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
        let mut buf: Vec<u16> = Vec::with_capacity(out.r.len() * 3);
        for i in 0..out.r.len() {
            buf.extend_from_slice(&[q(out.r[i]), q(out.g[i]), q(out.b[i])]);
        }
        let img = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(w, h, buf)
            .ok_or("16-bit buffer size mismatch")?;
        return img.save(out_path).map_err(|e| format!("cannot write image: {e}"));
    }
    out.to_rgb8().save(out_path).map_err(|e| format!("cannot write image: {e}"))
}

/// Largest image the app will process, in pixels. Above this the pipeline's buffers would exhaust memory.
const MAX_PIXELS: usize = 200_000_000;

/// Refuses images above the cap, with the size and the limit in the message.
fn check_size(w: usize, h: usize) -> Result<(), String> {
    if w * h > MAX_PIXELS {
        return Err(format!("image is too large to process ({} megapixels, limit {})", w * h / 1_000_000, MAX_PIXELS / 1_000_000));
    }
    Ok(())
}

/// Tile edge for export. Each tile is rendered with the margin its neighbourhood stages need, so the
/// stitched result matches a whole-image render.
const EXPORT_TILE: usize = 1024;

/// Renders the whole image tile by tile into one output, so only one tile's working buffers exist at a time.
fn process_tiled(full: &RgbImageF32, params: &AdjustParams, tile: usize) -> RgbImageF32 {
    let (w, h) = (full.width, full.height);
    let mut out = RgbImageF32::new(w, h);
    for y0 in (0..h).step_by(tile) {
        for x0 in (0..w).step_by(tile) {
            let tw = tile.min(w - x0);
            let th = tile.min(h - y0);
            let part = process_region(full, params, x0, y0, tw, th);
            for y in 0..th {
                let dst = (y0 + y) * w + x0;
                let src = y * tw;
                out.r[dst..dst + tw].copy_from_slice(&part.r[src..src + tw]);
                out.g[dst..dst + tw].copy_from_slice(&part.g[src..src + tw]);
                out.b[dst..dst + tw].copy_from_slice(&part.b[src..src + tw]);
            }
        }
    }
    out
}

/// Exact Quartz export at full resolution, at the source's bit depth.
fn export_quartz_sync(path: &str, out_path: &str, params: &AdjustParams) -> Result<(), String> {
    let (full, deep) = decode_full(path)?;
    check_size(full.width, full.height)?;
    let mut processed = process_tiled(&full, params, EXPORT_TILE);
    apply_brush_rgb(&mut processed, &params.brush)?;
    write_export(&processed, out_path, deep)
}

/// Exports each image in `paths` into `out_dir` with the same look and the source's file type where
/// it can be written (otherwise PNG), keeping file names. One failing file does not stop the rest;
/// returns one "path: reason" line per failure.
fn export_batch_sync(paths: &[String], out_dir: &str, params: &AdjustParams) -> Vec<String> {
    let mut failed = Vec::new();
    let mut used = std::collections::HashSet::new();
    for path in paths {
        let src = std::path::Path::new(path);
        let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .filter(|e| matches!(e.as_str(), "png" | "tif" | "tiff" | "jpg" | "jpeg"))
            .unwrap_or_else(|| "png".into());
        // Two sources with the same name (a.jpg, a.png) must not overwrite each other.
        let mut name = format!("{stem}.{ext}");
        let mut n = 2;
        while !used.insert(name.clone()) {
            name = format!("{stem}_{n}.{ext}");
            n += 1;
        }
        let out = std::path::Path::new(out_dir).join(&name);
        if let Err(e) = export_quartz_sync(path, &out.to_string_lossy(), params) {
            failed.push(format!("{path}: {e}"));
        }
    }
    failed
}

/// Batch export: the same look applied to each file, written into one folder. Returns the failures.
#[tauri::command]
async fn export_batch(paths: Vec<String>, out_dir: String, params: AdjustParams) -> Result<Vec<String>, String> {
    off_thread(move || Ok(export_batch_sync(&paths, &out_dir, &params))).await
}

/// Exact Quartz export at full resolution.
#[tauri::command]
async fn export_quartz(path: String, out_path: String, params: AdjustParams) -> Result<(), String> {
    off_thread(move || export_quartz_sync(&path, &out_path, &params)).await
}

/// Writes a preset (JSON text) to a path the user chose in the save dialog.
#[tauri::command]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    // Written beside the target and renamed, so a reader (the plug-in waiting on a hand-off) never
    // sees a half-written file.
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, contents).map_err(|e| format!("cannot write file: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write file: {e}"))
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

/// Writes the cancel marker when a hand-off is waiting for a result: the plug-in stops waiting and
/// Photoshop is released. With nothing pending this does nothing, and the plug-in clears a stale marker
/// when the next hand-off starts.
fn write_cancel_if_pending(dir: &std::path::Path) {
    if dir.join("handoff.ctrn").exists() && !dir.join("handoff.result.json").exists() {
        let _ = std::fs::write(dir.join("handoff.cancel"), "");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|_, event| {
            // Closing the window is a cancel: Photoshop is waiting for OK or Cancel, so it must be told.
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if let Some(home) = std::env::var_os("HOME") {
                    write_cancel_if_pending(&std::path::Path::new(&home).join("Library/Caches/Citrine"));
                }
            }
        })
        .manage(PendingOpen::default())
        .invoke_handler(tauri::generate_handler![
            render_preview, export_image, write_text_file, read_text_file, image_size, render_crop,
            quartz_presets, render_quartz_preview, render_quartz_crop, export_quartz, take_pending_open,
            render_base_preview, render_base_crop, curve_luts, preset_thumbnail, export_batch
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Quitting (Cmd+Q) also leaves a hand-off waiting, so it is cancelled the same way as closing.
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                if let Some(home) = std::env::var_os("HOME") {
                    write_cancel_if_pending(&std::path::Path::new(&home).join("Library/Caches/Citrine"));
                }
            }
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
    fn brush_dab_changes_only_its_neighbourhood_in_preview_and_export() {
        let src = RgbaImage::from_pixel(64, 64, image::Rgba([120, 120, 120, 255]));
        let mut p = AdjustParams::default();
        p.brush = vec![BrushDab { kind: 0, x: 0.5, y: 0.5, radius: 0.1, hardness: 0.5, opacity: 1.0, strength: 1.0, group: 1 }];
        let out = quartz_render(&src, &p);
        assert!(out.get_pixel(32, 32).0[0] > 120, "dodge should lighten the centre");
        assert_eq!(out.get_pixel(2, 2).0[0], 120);
        assert_eq!(out.get_pixel(32, 32).0[3], 255);

        let mut f = RgbImageF32::from_rgb8(&image::DynamicImage::ImageRgba8(src).to_rgb8());
        apply_brush_rgb(&mut f, &p.brush).unwrap();
        assert!(f.r[32 * 64 + 32] > f.r[2 * 64 + 2]);
    }

    #[test]
    fn batch_export_writes_each_file_and_reports_failures() {
        let dir = std::env::temp_dir().join(format!("photo-editor-batch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = temp_png("batch_a", 40, 30);
        let b = temp_png("batch_b", 40, 30);
        let missing = dir.join("nope.png").to_string_lossy().into_owned();
        let failed = export_batch_sync(&[a.clone(), b.clone(), missing.clone()], dir.to_str().unwrap(), &AdjustParams::default());
        assert!(dir.join("photo-editor-test-batch_a.png").exists());
        assert!(dir.join("photo-editor-test-batch_b.png").exists());
        assert_eq!(failed.len(), 1, "failures: {failed:?}");
        assert!(failed[0].starts_with(&missing));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(a);
        let _ = std::fs::remove_file(b);
    }

    #[test]
    fn neutral_preview_is_scaled_to_the_preview_size() {
        let src = temp_png("neutral", 1200, 600);
        let b64 = tauri::async_runtime::block_on(render_preview(src.clone(), Sliders::default())).unwrap();
        let decoded = image::load_from_memory(&STANDARD.decode(b64).unwrap()).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (960, 480));
        let _ = std::fs::remove_file(src);
    }

    #[test]
    fn export_at_full_size_is_identical_when_neutral() {
        let src = temp_png("export", 64, 32);
        let out = std::env::temp_dir().join("photo-editor-test-export.png");
        tauri::async_runtime::block_on(export_image(src.clone(), out.to_string_lossy().into_owned(), Sliders::default())).unwrap();
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
        assert!(tauri::async_runtime::block_on(render_preview("/nonexistent/x.png".into(), Sliders::default())).is_err());
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
        assert_eq!(presets.len(), 169);
        for p in presets {
            let out = quartz_render(&src, &p.params);
            assert_eq!(out.dimensions(), (64, 48), "preset {}", p.name);
            assert!(out.pixels().all(|px| px.0[3] == 255), "alpha changed in {}", p.name);
        }
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    fn temp(name: &str) -> String {
        std::env::temp_dir().join(format!("citrine_depth_{}_{name}", std::process::id())).to_string_lossy().into_owned()
    }

    /// A 16-bit gradient whose levels are not multiples of 257, so 8-bit storage would lose them.
    fn gradient16(path: &str) {
        let img = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_fn(64, 32, |x, y| {
            // Every pixel has its own red level: 64 x 32 distinct values.
            image::Rgb([((y * 64 + x) * 31 + 3) as u16, (y * 1500 + 7) as u16, (x * 700 + y * 300 + 11) as u16])
        });
        img.save(path).unwrap();
    }

    #[test]
    fn sixteen_bit_png_decodes_at_full_precision() {
        let p = temp("in.png");
        gradient16(&p);
        let (img, deep) = decode_full(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(deep);
        assert!((img.r[1] - 34.0 / 65535.0).abs() < 1e-6, "16-bit level kept");
    }

    #[test]
    fn sixteen_bit_tiff_decodes_at_full_precision() {
        let p = temp("in.tif");
        image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_fn(8, 4, |x, _| image::Rgb([(x * 1000 + 3) as u16, 0, 0]))
            .save(&p)
            .unwrap();
        let (img, deep) = decode_full(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(deep, "16-bit TIFF is recognised as deep");
        assert!((img.r[1] - 1003.0 / 65535.0).abs() < 1e-6);
    }

    #[test]
    fn sixteen_bit_export_keeps_levels_beyond_eight_bits() {
        let input = temp("src.png");
        let out = temp("out.png");
        gradient16(&input);
        let mut params = AdjustParams::default();
        params.basic_correction.exposure = 0.3;
        export_quartz_sync(&input, &out, &params).unwrap();
        let written = image::open(&out).unwrap();
        assert!(matches!(written.color(), image::ColorType::Rgb16), "16-bit output");
        let distinct: std::collections::HashSet<u16> = written.to_rgb16().pixels().map(|p| p[0]).collect();
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
        assert!(distinct.len() > 256, "more than 256 red levels survive");
    }

    #[test]
    fn eight_bit_source_exports_eight_bit() {
        let input = temp("src8.png");
        let out = temp("out8.png");
        image::RgbImage::from_fn(16, 16, |x, y| image::Rgb([(x * 15) as u8, (y * 15) as u8, 90])).save(&input).unwrap();
        export_quartz_sync(&input, &out, &AdjustParams::default()).unwrap();
        let written = image::open(&out).unwrap();
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
        assert!(matches!(written.color(), image::ColorType::Rgb8));
    }
}

#[cfg(test)]
mod raw16_tests {
    use super::*;

    #[test]
    fn sixteen_bit_handoff_decodes_at_full_precision() {
        let path = std::env::temp_dir().join(format!("citrine_raw16_{}.ctrn", std::process::id()));
        let mut bytes = b"CTR6".to_vec();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        for v in [1003u16, 20000, 65535, 3, 4, 5] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();
        let (img, deep) = decode_full(path.to_str().unwrap()).unwrap();
        let preview = load(path.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(deep);
        assert!((img.r[0] - 1003.0 / 65535.0).abs() < 1e-6);
        assert_eq!(preview.width(), 2);
    }
}

#[cfg(test)]
mod tiling_tests {
    use super::*;

    #[test]
    fn tiled_export_matches_the_whole_image_render() {
        let img = RgbImageF32::from_rgb8(&image::RgbImage::from_fn(300, 200, |x, y| {
            image::Rgb([(x * 37 % 256) as u8, (y * 53 % 256) as u8, ((x + y) * 11 % 256) as u8])
        }));
        let mut params = AdjustParams::default();
        params.details.strength = 0.6;
        params.adaptive_exposure.adaptive_exposure = 0.5;
        params.basic_correction.exposure = 0.2;
        let whole = process_image(&img, &params);
        let tiled = process_tiled(&img, &params, 128);
        let worst = whole.r.iter().zip(&tiled.r).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "tiles differ from the whole render by {worst}");
    }

    #[test]
    fn oversized_images_are_refused_with_a_clear_error() {
        assert!(check_size(1000, 1000).is_ok());
        let err = check_size(20_000, 20_000).unwrap_err();
        assert!(err.contains("too large") && err.contains("400 megapixels"), "message: {err}");
    }
}

#[cfg(test)]
mod close_tests {
    use super::*;

    #[test]
    fn closing_while_a_handoff_is_pending_writes_cancel() {
        let dir = std::env::temp_dir().join(format!("citrine_close_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("handoff.ctrn"), b"CTRN").unwrap();
        write_cancel_if_pending(&dir);
        let cancelled = dir.join("handoff.cancel").exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(cancelled);
    }

    #[test]
    fn closing_after_ok_or_without_a_handoff_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("citrine_close_ok_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("handoff.ctrn"), b"CTRN").unwrap();
        std::fs::write(dir.join("handoff.result.json"), b"{}").unwrap();
        write_cancel_if_pending(&dir);
        let after_ok = dir.join("handoff.cancel").exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!after_ok);
    }
}
