//! Tiled rendering with halo margins. The image is split into tiles; each tile is rendered with a
//! margin of its neighbours, so local filters see the same pixels as in a full-frame render. Effects
//! that depend on the whole image (adaptive target, colour mean, noise levels) are first measured
//! in streaming passes over the tiles, then every tile uses those values.
//!
//! Scope: the default Ours pipeline without strokes and without "process details independently".
//! Other cases render full-frame, and `apply_look_tiled` says so by doing that. Passes: target
//! (exposure and basic), detail noise, colour mean, denoise noise, then the final render.

use crate::adaptive_exposure::{log_luma_sum, AdaptCtx};
use crate::buffer::{EngineError, LinearImage};
use crate::color_adjust::{chroma_sum, ColorCtx};
use crate::details::{abs_histogram, sigma_from_histogram, SIGMA_BINS};
use crate::exposure::apply_exposure;
use crate::finishing::Region;
use crate::hdr::{self, Hdr, HdrCtx};
use crate::pipeline::{apply_look, Look, Style};

/// Whole-image values shared by every tile.
#[derive(Debug, Clone, Copy)]
struct Stats {
    target: f32,
    detail_sigma: Option<f32>,
    hdr_mean: f32,
    colour_mean: f32,
    denoise_sigma: Option<f32>,
}

/// A rectangle in image pixels.
#[derive(Debug, Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

/// Whole-image values and tile layout for one look and one source image. Compute it once, then
/// render the full image or any region from it, so a zoomed crop matches the full render.
pub struct Plan {
    look: Look,
    stats: Option<Stats>,
    halo: usize,
    full: (usize, usize),
    cores: Vec<Rect>,
}

impl Plan {
    /// Measures the whole-image values for `look` on `src`. Looks the tiled path does not cover
    /// (strokes, Quartz style, independent details) get a plan that renders full-frame.
    pub fn new(src: &LinearImage, look: &Look, tile: usize) -> Result<Self, EngineError> {
        let (w, h) = (src.width(), src.height());
        let full = (w, h);
        let tile = tile.max(1);
        let cores = tiles(w, h, tile);
        let halo = total_halo(look, full);
        if look.style != Style::Ours || !look.strokes.is_empty() || look.details.process_independently {
            return Ok(Self { look: look.clone(), stats: None, halo, full, cores });
        }

    // Pass 1: whole-image adaptive target, from exposure and basic only (both pointwise).
    let (mut sum, mut n) = (0f64, 0usize);
    for core in &cores {
        let mut t = crop(src, *core);
        apply_exposure(&mut t, look.ev)?;
        look.basic.apply(&mut t)?;
        let (s, c) = log_luma_sum(&t);
        sum += s;
        n += c;
    }
    let target = (sum / n as f64) as f32;
    let mut stats = Stats { target, detail_sigma: None, hdr_mean: 0.0, colour_mean: 0.0, denoise_sigma: None };

    // Pass 2: detail noise, measured on the adaptive output.
    let mut hist = vec![0u64; SIGMA_BINS];
    for core in &cores {
        let (mut t, inner) = with_halo(src, *core, halo, full);
        prefix(&mut t, look, full, &stats, Upto::Adaptive)?;
        let residue = look.details.residue(&t)?;
        add_core_hist(&mut hist, &residue, t.width(), inner);
    }
    stats.detail_sigma = Some(sigma_from_histogram(&hist));

    // Pass 3: HDR mean, on the detail output (only when HDR is on).
    if !look.hdr.is_neutral() {
        let (mut sum, mut n) = (0f64, 0usize);
        for core in &cores {
            let (mut t, inner) = with_halo(src, *core, halo, full);
            prefix(&mut t, look, full, &stats, Upto::Details)?;
            let (s, c) = hdr::log_luma_sum(&crop(&t, inner));
            sum += s;
            n += c;
        }
        stats.hdr_mean = (sum / n as f64) as f32;
    }

    // Pass 4: colour mean, on the HDR output.
    let (mut csum, mut cn) = (0f64, 0usize);
    for core in &cores {
        let (mut t, inner) = with_halo(src, *core, halo, full);
        prefix(&mut t, look, full, &stats, Upto::Hdr)?;
        let (s, c) = chroma_sum_core(&t, inner);
        csum += s;
        cn += c;
    }
    stats.colour_mean = (csum / cn as f64) as f32;

    // Pass 5: denoise noise, on the colour output.
    if look.denoise.amount > 0.0 {
        let mut hist = vec![0u64; SIGMA_BINS];
        for core in &cores {
            let (mut t, inner) = with_halo(src, *core, halo, full);
            prefix(&mut t, look, full, &stats, Upto::Color)?;
            let residue = crate::noise::residue(&t);
            add_core_hist(&mut hist, &residue, t.width(), inner);
        }
        stats.denoise_sigma = Some(sigma_from_histogram(&hist));
    }

        Ok(Self { look: look.clone(), stats: Some(stats), halo, full, cores })
    }

    /// Renders the whole image.
    pub fn render(&self, src: &LinearImage) -> Result<LinearImage, EngineError> {
        self.render_region(src, (0, 0, src.width(), src.height()))
    }

    /// Renders a rectangle `(x, y, w, h)` in image pixels. The result has the rectangle's size.
    pub fn render_region(&self, src: &LinearImage, rect: (usize, usize, usize, usize)) -> Result<LinearImage, EngineError> {
        let (rx, ry, rw, rh) = rect;
        let Some(stats) = self.stats else {
            let mut out = src.clone();
            apply_look(&mut out, &self.look)?;
            return Ok(crop(&out, Rect { x: rx, y: ry, w: rw, h: rh }));
        };
        let region = Rect { x: rx, y: ry, w: rw, h: rh };
        let mut out = LinearImage::new(rw, rh, vec![0.0; rw * rh * 4])?;
        for core in self.cores.iter().filter(|c| overlaps(c, &region)) {
            let (mut t, inner) = with_halo(src, *core, self.halo, self.full);
            let (original, _) = with_halo(src, *core, self.halo, self.full);
            prefix(&mut t, &self.look, self.full, &stats, Upto::Curve)?;
            let origin = (t_origin_x(*core, self.halo, self.full), t_origin_y(*core, self.halo, self.full));
            self.look.finishing.apply_region(&mut t, &original, Region { origin, full: self.full })?;
            paste_into(&mut out, &t, inner, *core, region);
        }
        Ok(out)
    }
}

/// Renders `src` with `look`, tile by tile. `tile` is the core size in pixels (at least 1).
pub fn apply_look_tiled(src: &LinearImage, look: &Look, tile: usize) -> Result<LinearImage, EngineError> {
    Plan::new(src, look, tile)?.render(src)
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Copies the part of a tile's core that lies inside `region` into `out` (which covers `region`).
fn paste_into(out: &mut LinearImage, t: &LinearImage, inner: Rect, core: Rect, region: Rect) {
    let x0 = core.x.max(region.x);
    let y0 = core.y.max(region.y);
    let x1 = (core.x + core.w).min(region.x + region.w);
    let y1 = (core.y + core.h).min(region.y + region.h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let ow = out.width();
    let tw = t.width();
    let data = out.data_mut();
    for y in y0..y1 {
        let src_row = ((inner.y + (y - core.y)) * tw + inner.x + (x0 - core.x)) * 4;
        let dst_row = ((y - region.y) * ow + (x0 - region.x)) * 4;
        let n = (x1 - x0) * 4;
        data[dst_row..dst_row + n].copy_from_slice(&t.data()[src_row..src_row + n]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
enum Upto {
    Adaptive,
    Details,
    Hdr,
    Color,
    Curve,
}

/// Runs the stages in order, up to and including `upto`, with the whole-image values in `stats`.
fn prefix(t: &mut LinearImage, look: &Look, full: (usize, usize), stats: &Stats, upto: Upto) -> Result<(), EngineError> {
    apply_exposure(t, look.ev)?;
    look.basic.apply(t)?;
    look.adaptive.apply_with(t, Some(AdaptCtx { target: stats.target, full_dims: full }))?;
    if upto == Upto::Adaptive {
        return Ok(());
    }
    let residue = look.details.residue(t)?;
    look.details.apply_residue_with(t, &residue, stats.detail_sigma);
    if upto == Upto::Details {
        return Ok(());
    }
    look.hdr.apply_with(t, Some(HdrCtx { mean: stats.hdr_mean, full_dims: full }));
    if upto == Upto::Hdr {
        return Ok(());
    }
    look.color.apply_with(t, Some(ColorCtx { global_mean: stats.colour_mean, full_dims: full }))?;
    if upto == Upto::Color {
        return Ok(());
    }
    look.denoise.apply_with(t, stats.denoise_sigma)?;
    look.curve.apply(t)?;
    Ok(())
}

/// Sum of all stage halos, so that every chained blur in a tile sees its full neighbourhood.
fn total_halo(look: &Look, full: (usize, usize)) -> usize {
    let (fw, fh) = full;
    let maxd = fw.max(fh);
    let regions = look.adaptive.regions.clamp(1, 64) as f32;
    let adapt_r = ((maxd as f32 / regions) * 0.5).round().max(1.0) as usize;
    let detail_r = look.details.radius.clamp(1.0, 16.0).round() as usize;
    let colour_r = ((maxd as f32 / (16.0 * look.color.color_regions.clamp(1.0, 10.0))).round() as usize).max(1);
    let mut halo = 3 * adapt_r + 2; // adaptive guided filter
    halo += 3 * detail_r + 2; // details residue
    if !look.hdr.is_neutral() {
        halo += 3 * Hdr::radius(full) + 2; // HDR base filter
    }
    halo += colour_r + 1; // colour box mean
    halo += 3 * 2 + 2; // denoise guided filter (radius 2)
    if look.finishing.diffusion_softness > 0.0 {
        halo += 3 * (maxd / 100).max(1) + 2; // softness
    }
    if look.finishing.diffusion > 0.0 {
        halo += (maxd / 50).max(1) + 1; // bloom
    }
    halo
}

/// Core rectangles covering the image, row by row.
fn tiles(w: usize, h: usize, tile: usize) -> Vec<Rect> {
    let mut out = Vec::new();
    for y in (0..h).step_by(tile) {
        for x in (0..w).step_by(tile) {
            out.push(Rect { x, y, w: tile.min(w - x), h: tile.min(h - y) });
        }
    }
    out
}

/// The core plus `halo` pixels on each side, clipped to the image. Returns the tile image and the
/// position of the core inside it.
fn with_halo(src: &LinearImage, core: Rect, halo: usize, _full: (usize, usize)) -> (LinearImage, Rect) {
    let (sw, sh) = (src.width(), src.height());
    let x0 = core.x.saturating_sub(halo);
    let y0 = core.y.saturating_sub(halo);
    let x1 = (core.x + core.w + halo).min(sw);
    let y1 = (core.y + core.h + halo).min(sh);
    let rect = Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    let inner = Rect { x: core.x - x0, y: core.y - y0, w: core.w, h: core.h };
    (crop(src, rect), inner)
}

fn crop(src: &LinearImage, r: Rect) -> LinearImage {
    let sw = src.width();
    let mut data = Vec::with_capacity(r.w * r.h * 4);
    for y in r.y..r.y + r.h {
        let start = (y * sw + r.x) * 4;
        data.extend_from_slice(&src.data()[start..start + r.w * 4]);
    }
    LinearImage::new(r.w, r.h, data).expect("crop dimensions are valid")
}

fn t_origin_x(core: Rect, halo: usize, _full: (usize, usize)) -> usize {
    core.x.saturating_sub(halo)
}

fn t_origin_y(core: Rect, halo: usize, _full: (usize, usize)) -> usize {
    core.y.saturating_sub(halo)
}

/// Adds the |residue| histogram of the core pixels only.
fn add_core_hist(hist: &mut [u64], residue: &[f32], tile_w: usize, inner: Rect) {
    let h = abs_histogram(&crop_values(residue, tile_w, inner));
    for (a, b) in hist.iter_mut().zip(h) {
        *a += b;
    }
}

/// Values of a per-pixel array over a rectangle.
fn crop_values(values: &[f32], tile_w: usize, r: Rect) -> Vec<f32> {
    let mut out = Vec::with_capacity(r.w * r.h);
    for y in r.y..r.y + r.h {
        out.extend_from_slice(&values[y * tile_w + r.x..y * tile_w + r.x + r.w]);
    }
    out
}

fn chroma_sum_core(t: &LinearImage, inner: Rect) -> (f64, usize) {
    chroma_sum(&crop(t, inner))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive_exposure::AdaptiveExposure;
    use crate::color_adjust::ColorAdjust;
    use crate::curve::Curve;
    use crate::details::Details;
    use crate::finishing::Finishing;
    use crate::noise::Denoise;

    fn scene(w: usize, h: usize) -> LinearImage {
        let mut s: u32 = 77;
        let mut data = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let n = (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                let base = 0.1 + 0.7 * x as f32 / w as f32 + 0.1 * ((y / 20) % 2) as f32;
                let v = (base + 0.03 * n).clamp(0.0, 1.0);
                data.extend_from_slice(&[v, v * 0.8 + 0.1, v * 0.6 + 0.05, 1.0]);
            }
        }
        LinearImage::new(w, h, data).unwrap()
    }

    fn busy_look() -> Look {
        Look {
            ev: 0.2,
            basic: crate::basic_exposure::BasicExposure { contrast: 0.2, boost_blacks: 0.1, boost_whites: 0.0 },
            adaptive: AdaptiveExposure { strength: 0.7, regions: 6, contrast: 0.1, brightness_ev: 0.0, protect_highlights: 0.2, protect_shadows: 0.2 },
            details: Details { strength: 1.2, boost: 1.2, threshold: 0.3, radius: 2.0, sharpen: 0.5, process_independently: false },
            color: ColorAdjust { adaptive_saturation: 0.4, saturation: 1.2, boost: 0.2, hue_deg: 5.0, color_regions: 3.0 },
            hdr: crate::hdr::Hdr { strength: 0.5, detail: 1.3 },
            denoise: Denoise { amount: 0.5, suppression: 1.0 },
            curve: Curve { points: vec![(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)] },
            finishing: Finishing {
                warmth: 0.2,
                diffusion: 0.3,
                diffusion_softness: 0.2,
                vignette_strength: 0.4,
                grain_amount: 0.2,
                grain_seed: 9,
                border_size: 0.03,
                border_color: [0.9, 0.9, 0.9],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn max_diff(a: &LinearImage, b: &LinearImage) -> f32 {
        a.data().iter().zip(b.data()).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max)
    }

    #[test]
    fn tiled_render_matches_full_frame() {
        let src = scene(150, 110);
        let look = busy_look();
        let mut full = src.clone();
        apply_look(&mut full, &look).unwrap();
        for tile in [32usize, 47, 150] {
            let tiled = apply_look_tiled(&src, &look, tile).unwrap();
            let d = max_diff(&tiled, &full);
            assert!(d < 2e-3, "tile {tile}: tiled differs from full frame by {d}");
        }
    }

    #[test]
    fn region_render_matches_crop_of_full_render() {
        // A zoomed crop must match the same pixels of the full render, across tile borders.
        let src = scene(150, 110);
        let look = busy_look();
        let plan = Plan::new(&src, &look, 32).unwrap();
        let full = plan.render(&src).unwrap();
        let (x, y, w, h) = (20usize, 25usize, 70usize, 50usize);
        let region = plan.render_region(&src, (x, y, w, h)).unwrap();
        assert_eq!((region.width(), region.height()), (w, h));
        let mut crop_full = Vec::new();
        for yy in y..y + h {
            let start = (yy * 150 + x) * 4;
            crop_full.extend_from_slice(&full.data()[start..start + w * 4]);
        }
        let expect = LinearImage::new(w, h, crop_full).unwrap();
        let d = max_diff(&region, &expect);
        assert!(d < 1e-6, "region differs from the full-render crop by {d}");
    }

    #[test]
    fn neutral_look_is_identical_tiled() {
        let src = scene(40, 30);
        let out = apply_look_tiled(&src, &Look::default(), 16).unwrap();
        assert_eq!(out, src);
    }
}
