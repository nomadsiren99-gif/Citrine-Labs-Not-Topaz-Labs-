# Feature matrix

Status values: **DONE** (code, tests run and passed), **WRITTEN** (code exists, not compiled or tested), **PARTIAL**, **NOT STARTED**, **BLOCKED**.

Source of truth for feature names: the Topaz Adjust 5 manual, as described in the spec. Parameter ranges and defaults are **unverified** unless noted.

| Module | Control | Status | Notes |
|---|---|---|---|
| Global exposure | EV gain (2^EV) | DONE (unit tests and CLI pixel check pass) | `engine::apply_exposure`, +/-10 EV clamp. Not a historical Adjust 5 control; it is the first kernel only. |
| Adaptive Exposure | Adaptive Exposure, Regions, Contrast, Brightness, Protect Highlights, Protect Shadows | NOT STARTED | Next kernel. Local log-luminance model per spec section 18.2. |
| Details | Strength, Boost, Threshold, Radius, Sharpen, Process Details Independently | NOT STARTED | |
| Color | Adaptive Saturation, Color Regions, Saturation, Saturation Boost, Hue | NOT STARTED | Work in OKLCH per spec. |
| Noise | Suppression, Amount, high-quality mode | NOT STARTED | Built-in denoiser replaces the Topaz DeNoise checkbox; label as a substitute. |
| Curves | Points, presets, reset | NOT STARTED | |
| Local | Dodge, Burn, Brush Out, Smooth, strength, size, opacity, hardness, edge aware | NOT STARTED | Hardness is inverted in the original (0 hard, 1 soft); keep that. |
| Finishing | Diffusion, Grain, Transparency, Warmth, Border, Vignette, four-zone Tone | NOT STARTED | |
| Presets and editor | Collections, stacking, snapshots, histogram, undo/redo | NOT STARTED | |
| Photoshop | Native Smart Filter | BLOCKED | See `PHOTOSHOP_INTEGRATION.md`. |
| I/O | PNG, JPEG read/write | DONE for 8-bit PNG (CLI tested); JPEG not yet tested | 8-bit sRGB only. ICC and 16-bit are not handled. |
| I/O | TIFF, 16-bit, ICC, EXIF orientation | NOT STARTED | |
