# Feature matrix

Status values: **DONE** (code, tests run and passed), **WRITTEN** (code exists, not compiled or tested), **PARTIAL**, **NOT STARTED**, **BLOCKED**.

Source of truth for feature names: the Topaz Adjust 5 manual, as described in the spec. Parameter ranges and defaults are **unverified** unless noted.

| Module | Control | Status | Notes |
|---|---|---|---|
| Global exposure | EV gain (2^EV) | DONE (unit tests and CLI pixel check pass) | `engine::apply_exposure`, +/-10 EV clamp. Not a historical Adjust 5 control; it is the first kernel only. |
| Adaptive Exposure | Adaptive Exposure, Regions, Contrast, Brightness, Protect Highlights, Protect Shadows | WRITTEN, unit tests pass (v1) | `engine::AdaptiveExposure`. Box-blur local mean of log2 luminance, lift toward the global mean. Mappings are proposed, not verified against Topaz. No tiles, no GPU, no CLI flag yet. |
| Details | Strength, Boost, Threshold, Radius, Sharpen | WRITTEN, unit tests pass (v1, direction tests for every control) | `engine::Details`. Box-blur base, gains on detail, halo guard at +/-1 EV. Process Details Independently done in `engine::pipeline::apply` (detail taken before tone mapping). Mappings proposed. CLI: `--details`. |
| Basic Exposure | Brightness, Contrast, Boost Blacks, Boost Whites | WRITTEN, unit tests pass (v1) | `engine::BasicExposure` (brightness is the look's EV). |
| Color | Adaptive Saturation, Color Regions, Saturation, Saturation Boost, Hue | WRITTEN, unit tests pass (v1, Color Regions added as a region-scale control) | `engine::ColorAdjust` in OKLab. Adaptive Saturation (local vs global chroma), Saturation, Boost (low chroma only), Hue rotation. Color Regions not done. Gamut is a simple clamp at 0 (shortcut: no gamut compression, upgrade before release). Mappings proposed. |
| Noise | Suppression, Amount, high-quality mode | WRITTEN, tests pass (v1) | `engine::Denoise`: deviations below a threshold from a blurred base are reduced. Edges are kept. High-quality mode not done. Label as our own denoiser, not Topaz DeNoise. |
| Curves | Points, presets, reset | WRITTEN, tests pass (v1) | `engine::Curve`: monotone cubic through points, 1024 LUT, applied to luminance so hue is kept. UI has three output-level sliders (shadows, mids, highlights), not a draggable curve. Presets not done. |
| Local | Dodge, Burn, Brush Out, Smooth, strength, size, opacity, hardness, edge aware | PARTIAL, tests pass (v1) | `engine::local`: soft circular strokes with kind, size, hardness, opacity and strength. No brush UI, no persistent masks, no edge-aware mask. Hardness uses our own direction; the original is inverted, unverified. |
| Finishing | Diffusion, Grain, Transparency, Warmth, Border, Vignette, four-zone Tone | WRITTEN, tests pass (v1) | `engine::Finishing`, all seven controls. Grain is seeded by coordinates, so tiles match. Border is an overlay, not a canvas extension. All mappings proposed. |
| Presets and editor | Collections, stacking, snapshots, histogram, undo/redo | PARTIAL: UI has 3 of our own presets, undo/redo, zoom, histogram, sections with reset, drag-and-drop open. No collections, snapshots, or preset save yet. | Presets must be our own. Topaz preset names and values are not to be used. |
| Photoshop | Native Smart Filter | BLOCKED | See `PHOTOSHOP_INTEGRATION.md`. |
| I/O | PNG, JPEG read/write | DONE for 8-bit PNG (CLI tested); JPEG not yet tested | 8-bit sRGB only. ICC and 16-bit are not handled. |
| I/O | TIFF, 16-bit, ICC, EXIF orientation | NOT STARTED | |

Note (2026-10-10): adaptive exposure, Details and noise suppression now use the guided filter (ported from the author's Quartz repo). Details tests now target texture. UI has Single, Split Wipe and Side-by-Side views and preset search, following the Quartz panel ideas in our own styling. Quartz's tone-mapping gamma, detail weight curves and spline curves are not ported.

Note (2026-10-10, later): the Quartz stage set is in the engine and selectable. 21 factory presets are included under our own names, with the derivation risk recorded in D10.
