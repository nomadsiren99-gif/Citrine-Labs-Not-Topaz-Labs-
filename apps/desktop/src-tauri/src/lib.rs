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
        _ => StrokeKind::BrushOut,
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
    // Use the Limestone stage set instead of the default engine
    pub limestone: bool,
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
            details: 1.0, detail_boost: 1.0, detail_threshold: 0.5, detail_radius: 1.5, detail_sharpen: 0.0,
            process_independently: false,
            adaptive_saturation: 0.0, saturation: 1.0, saturation_boost: 0.0, hue: 0.0, color_regions: 1.0,
            denoise: 0.0, noise_suppression: 1.0,
            curve_shadows: 0.0, curve_mids: 0.5, curve_highlights: 1.0,
            warmth: 0.0, tone_colors: [[1.0; 3]; 4], tone_strength: 0.0, diffusion: 0.0,
            vignette: 0.0, vignette_size: 0.5, vignette_transition: 0.5, vignette_center_x: 0.5, vignette_center_y: 0.5,
            grain: 0.0, grain_size: 1.0, grain_seed: 1, border_size: 0.0, border_color: [0.0; 3], transparency: 0.0,
            limestone: false,
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
            style: if self.limestone { Style::Limestone } else { Style::Ours },
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
                })
                .into_iter()
                .chain(self.brush.iter().map(|d| Stroke {
                    kind: kind_of(d.kind),
                    center: [d.x, d.y],
                    radius: d.radius,
                    hardness: d.hardness,
                    opacity: d.opacity,
                    overall_strength: d.strength,
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
            },
        }
    }
}

fn load(path: &str) -> Result<RgbaImage, String> {
    image::open(path).map(|i| i.to_rgba8()).map_err(|e| format!("cannot open image: {e}"))
}

fn render(src: &RgbaImage, look: &Look) -> Result<RgbaImage, String> {
    let (w, h) = src.dimensions();
    let mut img = LinearImage::from_srgb8(w as usize, h as usize, src.as_raw()).map_err(|e| e.to_string())?;
    apply_look(&mut img, look).map_err(|e| e.to_string())?;
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
#[tauri::command]
fn export_image(path: String, out_path: String, sliders: Sliders) -> Result<(), String> {
    let full = load(&path)?;
    let out = render(&full, &sliders.look())?;
    out.save(&out_path).map_err(|e| format!("cannot write image: {e}"))
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![render_preview, export_image, write_text_file, read_text_file])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
}
