# Image quality fixes: first three defects

Repo: `photo-editor-filter-rebuild` (copy of `photo-editor` at commit `13864f8`; the original is untouched).

Target reference: Topaz Adjust 5 (5.1.0) control vocabulary. All coefficients, ranges and thresholds below are **proposed**, not Topaz originals.

## How the defects were found

`crates/engine/examples/quality_probe.rs` renders deterministic synthetic fixtures (ramp, backlit tree, flat noise, saturated patches with a bright orange highlight test, photo-like texture, 1 px checker) through both paths:

- **Limestone** (`limestone_exact`), the desktop UI's default mode.
- **Ours** (`apply_look`), used by `render-cli` and the Ours mode.

Settings are the user's own "Detail - Medium" readouts from the screenshots (adaptive 1.0, details strength 1.07, boost 1.30, sharpen 1.0). Fixtures are generated, so no rights-cleared photos were needed.

## Defects, measured on the pristine commit

| # | Defect | Measurement (pristine) | Cause |
|---|---|---|---|
| 1 | Ours adaptive exposure flattens the image | tonal range 0.12x, texture retention 0.54 | The guided-filter base had `eps = 0.02²` on log₂ luma, so it tracked every pixel. The lift became `target − pixel` and pulled everything to one grey. At full strength the whole log range closed to the global mean. |
| 2 | Details amplify noise | flat-patch noise 1.93x (Limestone), 1.65x (Ours) | Boost (up to 2x) and sharpen add gain to residue at noise level. Nothing gates the gain, and the boost is applied to every low-amplitude texture, which includes sensor noise. |
| 3 | Preview and export disagree on detail | mean code diff 0.21 (Limestone), 0.45 (Ours) | Largely a side effect of #2. The noise was the part that changed with resolution. The underlying issue (detail radius in fixed pixels, not scaled to image size) is **not fixed**. |

The Limestone adaptive stage is not the cause of #1; its tonal range is 0.81 and it keeps texture at 0.89.

## Fixes

1. **`crates/engine/src/adaptive_exposure.rs`**
   - `BASE_EPS = 0.3²`: texture below about 0.3 stop is smoothed out of the illumination base. Light and shadow steps are still kept, so edges hold.
   - `TONE_COMPRESSION = 0.5`: full strength closes half the log gap to the global mean, not all of it.
   - Regression tests: `fine_texture_survives_full_lift`, `full_strength_keeps_most_of_the_tonal_range`.
2. **`crates/engine/src/details.rs`** (Ours) and **`crates/engine/src/limestone_exact/stages/details.rs`** (Limestone)
   - `noise_sigma()`: robust estimate, `1.4826 · median(|d|)`, computed per image. Shortcut: one global estimate, not per region.
   - Amplifying gain (boost, sharpen) is multiplied by `smoothstep(2σ, 4σ, |d|)`. Smoothing (strength < 1) is not gated.
   - Regression tests: `boost_and_sharpen_do_not_amplify_flat_noise` in both files.

## Results (256×256 fixtures, `quality/`)

| Metric | Pristine | Fixed |
|---|---|---|
| Ours: tonal range (std after/before) | 0.12 | 0.53 |
| Ours: texture retention | 0.54 | 0.73 |
| Ours: halo, sky side of tree edge (code values) | 0.00 | 0.79 (new, small) |
| Ours: flat-patch noise ratio | 1.65 | 1.00 |
| Ours: preview vs export mean code diff | 0.45 | 0.05 |
| Limestone: flat-patch noise ratio | 1.93 | 1.00 |
| Limestone: preview vs export mean code diff | 0.21 | 0.00 |
| Limestone: tonal range / texture retention | 0.81 / 0.89 | 0.81 / 0.89 (unchanged) |

Engine tests: 73 passing before, 77 passing after. The four new tests are the regressions listed above.

Galleries (input | before | after, same crop):
- `quality/gallery_ours_adaptive_photo.png`
- `quality/gallery_ours_adaptive_backlit.png`
- `quality/gallery_ls_adaptive_photo.png`
- `quality/gallery_detail_flat_noise_100pct.png` (Limestone, 100%)
- `quality/gallery_ours_detail_flat_noise_100pct.png` (Ours, 100%)

Reproduce: `cargo run -p engine --release --example quality_probe -- quality after`

## Open issues, not fixed

- **Limestone is not a clean port.** Its code and presets are taken from `anuzdhk/Limestone`, which has no licence, and the presets copy Topaz Adjust 4 looks. The memory rule says this may be used as an idea reference only. This needs a decision from the owner before shipping. I did not remove it.
- **Limestone still processes gamma-encoded sRGB.** `RgbImageF32` says "linear" but holds encoded values, so gains and luma are not photometric. Not changed here.
- **Limestone details radius is in fixed pixels**, so preview and export scale differently for large radii (the 30 px from the screenshot). Not changed.
- **Ours tonal range 0.53 at full strength** is still a strong compression. Whether that matches Adjust 5 needs a comparison against the real product, which is not available on this Mac.
- **Ours shows a small new halo** (0.79 code values) at the tree edge, from the smoother base.
- Spec gates 0, 3–7 (colour, curves, local tools, finishing, presets, GPU parity) are not done. No 16-bit, ICC or alpha tests were added.
- `cargo fmt --check` already fails on the pristine commit; I did not reformat unrelated code.
- `FILTER_AUDIT.md` (full per-control audit) is not written yet.

## Second milestone: colour gamut and GPU kernel

- **Colour gamut (`color_adjust.rs`, `fit_gamut`)**: the per-channel `max(0)` clamp after a saturation boost shifted hue near the sRGB gamut edge. Chroma is now reduced at constant lightness and hue until the colour fits. Regression test `strong_saturation_keeps_hue_inside_gamut`: it fails with the old clamp (hue moved −18°) and passes with the fit.
- **GPU (`gpu.rs`, wgpu 30 + WGSL)**: separable box blur compute kernel, the building block of the guided filter. It is verified against the CPU blur on this Mac's Metal adapter (`gpu_box_blur_matches_cpu`, tolerance 1e-4). The guided filter and the rest of the stages still run on the CPU. Shortcut: each output sums its taps directly (O(r)), not a running sum.
- Engine tests: 79 passing.

## Not done

- Trained small models: none is cleared for use yet (see the message to the user). Nothing was downloaded.
- GPU is not yet wired into the adaptive, details or noise stages.

## Third milestone: Gate 0, accurate I/O (partial)

- **16-bit path** (`buffer.rs`): `from_srgb16` / `to_srgb16`. Every one of the 65,536 code values round-trips exactly, and alpha is kept exactly (tests `every_16bit_value_round_trips_exactly`, `alpha_is_kept_exactly_in_16_bit`).
- **CLI bit depth** (`render-cli`): 16-bit PNG in gives 16-bit PNG out. 8-bit stays 8-bit. Integration test `neutral_16_bit_png_round_trips_exactly` runs the real binary on a 256×256 RGBA16 image with neutral settings and requires every sample to match.
- **Not done under Gate 0:** ICC profiles are still ignored (treated as sRGB). The desktop app still loads 8-bit only. TIFF input is untested.
- Engine tests: 81 passing. render-cli: 1 integration test passing.

## Fourth milestone: Gate 4, noise and curves

- **Denoise (`noise.rs`)**: replaced the hard cut at a fixed 0.05 log2 level with a smooth weight scaled to the image's own noise estimate (`noise_sigma`). Full correction below 2σ, zero by 4σ. The first version of the test measured flat-patch variance at 0.98x, so the old cut was doing almost nothing on this noise. Test `flat_patch_variance_drops_and_step_edge_survives`: flat-patch std falls below 0.7x, and a strong step keeps over 90% of its contrast.
- **Curve (`curve.rs`)**: property test over 200 random monotone point sets. Output stays monotone and finite. A gentle curve near identity has no adjacent-pixel jumps above 0.02. A test requiring no jumps at all was dropped: steep random curves can legitimately jump, so it was the wrong check.
- **Not checked yet**: the Limestone denoise stage (`limestone_exact/stages/denoise.rs`) has not been measured against Gate 4.
- Engine tests: 84 passing.

## Fifth milestone: Gate 4 on the Limestone path

- **Limestone denoise** (`limestone_exact/stages/denoise.rs`): test `flat_patch_variance_drops_and_step_edge_survives`. Measured: flat-patch std 0.23× (threshold below 0.7×), step edge keeps 97% of its contrast (threshold above 90%). No code change needed.
- **Open deviation**: this stage uses `amount` as the filter radius (`radius = amount × 5`). The spec defines Amount as the blend between denoised and original. The mismatch is recorded; not changed here.
- Gate 4 now covers both denoise paths and curves. The Ours curve test covers monotonicity only.

## Sixth milestone: remaining gate checks (Gates 1, 2, 3) and Gate 5 (partial)

**Gate 1 (core tone)**
- Adaptive exposure halo at a dark tree against bright sky: 1.7% of the edge step, guard 3%. The guard sits just above the measured value, so it is a regression check, not proof of quality.

**Gate 2 (detail)**
- Ringing (overshoot past the far plateau): Ours default 0.00%, medium 0.76%, strong 0.96%; Limestone 0.08%. Guard 2%.
- Preview versus export detail radius (fixed pixels, not scaled to image size): still open. The probe shows the difference is negligible on the 256 px fixture, but it is not tested at large radius or large image size.

**Gate 3 (colour)**
- Neutral greys stay neutral (chroma below 0.002) at saturation 2.0 with adaptive saturation 1.0.
- Skin hue stays within 3° at saturation 0.5 to 1.6.

**Gate 5 (local tools and finishing)**
- **Warmth (bug fixed)**: the old warmth was a red-up, blue-down multiplier. It is now a Bradford chromatic adaptation from D65 to a colour temperature (6500 K at zero, 4000 K at +1, 9000 K at −1), applied as a linear-RGB matrix. Zero is still an exact identity.
- **Kim approximation (bug caught by the reference test)**: the 4000 K-and-up coefficient set was missing and the wrong set was used. Fixed. At 4000 K the result is R/G 1.50 and B/G 0.52 to 0.58, against the CIE reference of 1.53 and 0.58.
- **Hardness (bug fixed)**: hardness was inverted, so 1 gave the softest edge. Now 1 is the sharpest edge and 0 the softest, as documented. Edge width tests added.
- **Vignette**: darkening capped at 80%, so the corners bottom out at a gain of 0.2 instead of black. Test: corners stay above 0.1 and the falloff step stays below 0.05 per pixel.
- **Smooth stroke**: removes texture inside its mask only (test). Strokes are deterministic (test).

**Gate 5 still open**
- Grain colour (chromatic grain correlation) is not implemented.
- Diffusion transition (smooth threshold on the highlight mask) is not implemented.
- Edge-aware local masks are not implemented.
- Mask view, eraser and undo are UI features. The engine-side stroke list is deterministic, but the brush UI is not built.
- Border and tone-colour zones are not tested against the spec.

Engine tests: 97 passing. render-cli: 1 integration test passing.

## Seventh milestone: Gate 5 open items (grain colour, diffusion transition)

- **Grain colour** (`grain_color`, 0..1, default 0): mixes the shared grain value with an independent per-channel value. Test: monochrome grain has R/G correlation above 0.99; full grain colour is below 0.3 in magnitude.
- **Diffusion transition** (`diffusion_transition`, 0..1, default 0.5): sets the width of the highlight falloff, from 0.15 to 0.50 luma. The narrowest setting first gave a 0.05 per-pixel jump in the bloom, which is a hard cutoff, so the minimum width was raised to 0.15. Test: the per-pixel bloom step stays under 0.03 at every setting, and the wider setting gives a gentler slope.
- **Desktop app**: the new fields are not exposed in the UI yet. The desktop mapping uses `..Default::default()`, so they stay neutral. `cargo check` on the desktop crate passes.
- Engine tests: 99 passing.
- **Still open in Gate 5**: edge-aware local masks, the brush UI (mask view, eraser, undo), and tests for borders and tone-colour zones.

## Eighth milestone: stock photographs and Gate 6 (preset recipes, first pass)

**Stock test photographs** (`quality/stock/`, licences in `CREDITS.md`, all from Wikimedia Commons, licences read from the Commons API before download):
- `backlit_joshua_tree.jpg` (public domain), `backlit_birch_tree.jpg` (CC BY-SA 4.0), `forest_floor_leaves.jpg` (CC BY 4.0), `train_interior.jpg` (CC BY 4.0).
- No CC0 or public-domain photographic portraits or night photos were found. Commons results for those were paintings or CC BY licences. Gaps: real skin, night noise.

**Gate 0 on real photos**: with the same decoder, a neutral render is bit-identical on all four photos (zero changed samples across roughly 118 million samples). The earlier difference against Pillow came from the two JPEG decoders, not from the engine.

**Gate 6 harness** (`crates/engine/examples/preset_gallery.rs`): renders every recipe on each photo (1000 px long side), writes per-photo contact sheets and `metrics.csv`.
- The spec table holds **42** recipes, not the 39 in the gate text. All 42 are rendered.
- Recipes use neutral IDs (R01 to R42) and objective descriptions. The legacy Topaz names are not used, per the memory rule.
- Mapping from normalised UI values to engine parameters is documented at the top of the file. It is a proposal.
- Tonal tone-anchor recipes (R31, R34, R41, R42) and posterize (R34) are **not implemented**. Their results omit the tone or posterize step.

**Automatic results**
- Clipping is luminance-based. An earlier "any channel" count flagged in-gamut saturated colours, so it was replaced. The largest luminance clip added is 0.12% of pixels.
- Crushing: the dark recipes (R38, R40) add 1 to 2% of pixels in shadows, as intended for a low-key look.
- Texture (residual std, against input): the strong adaptive recipes lower texture by 22 to 31% (R10, R24, R25). No recipe raises texture above 1.02×.

**Defect found and partly fixed: low-chroma tint amplification (Gate 3)**
- A bluish rock region (input B−G = +3.3) rose to +10.1 under R18 and +7.7 under R20. Low-chroma pixels were being boosted by up to 2×.
- Fix (`color_adjust.rs`): boost has a floor (no effect below OKLab chroma 0.01, fading in by 0.04), and adaptive plus boost may add at most 0.05 chroma per pixel. Result: R18 rock B−G is +8.2, R20 is +6.3.
- **Still open**: the remaining rise comes from the explicit global saturation setting (×1.95 in R18, ×1.48 in R20). It scales any faint cast. A neutral-preserving guard, such as automatic white balance or a chroma-dependent saturation roll-off, is needed. Not done.

**Visual grading**: contact sheets are in `quality/presets/sheet_*.png`. Grading is by eye and is not complete. Recipes R18 and R20 show the purple cast above. Remaining recipes look plausible on the Joshua tree sheet.

**Not done for Gate 6**: a portrait photo with real skin, a night photo, presets rated against a target style, and the recipe revisions that the grading would drive.

## Ninth milestone: night and portrait photos, skin protection, and decision on creative looks

**Supplied photos** (`quality/stock/night_skyline.jpg`, `portrait_studio.jpg`): licence unknown, internal test use only. They cover the night and skin gaps noted earlier.
- Night: no texture rise under any recipe (maximum 1.00). Dark recipes crush 12–19% of pixels, as intended for low-key looks.
- Portrait: before the fix, R34 and R35 warmed the face, and R18 turned it orange.

**Changes** (`color_adjust.rs`)
- Skin protection: chroma increases are scaled down by up to 70% on skin hues (OKLab 20 to 90°). Hue rotation is scaled down the same way.
- Tests: `skin_gains_less_chroma_than_a_non_skin_colour` (gain 1.8× on a non-skin colour versus skin); the whole engine suite passes at 103.
- Measured face warmth: R18 blue-minus-green moves from −31.7 to −24.6 (input −19.5). R34 red-minus-green is still +56.5 (input +50.9), about 11% more chroma on skin.

**Decision (owner, this session)**: the software is a filter tool, so creative recipes may stylise skin. The remaining orange in R18 and R34 is accepted and not tuned further. Skin protection stays on as a default.

**Status**: Gate 6 first pass is done for the six stock photos. Visual grading is by eye, and the contact sheets are in `quality/presets/`. The 42 recipes use the proposed mapping, not Topaz values.

## Tenth milestone: Gate 7, GPU parity (partial)

- **Box blur on the GPU** (`gpu.rs`, wgpu 30, WGSL): now a running-sum kernel, one thread per row and per column. Cost no longer grows with the radius. The first version summed each window directly, and that was slower than the CPU at radius 30 and above.
- **Guided filter on the GPU**: the six box blurs run on the GPU, and the per-pixel steps stay on the CPU.
- **Parity**: on a 9.8 MP photo the largest difference from the CPU is 6e-5 at radii 8, 30 and 100. Unit tests check the blur (1e-4) and the guided filter (1e-3).
- **Speed** (Apple M4, the 3840×2560 Joshua tree photo, luma): CPU 206, 179 and 176 ms; GPU 115, 112 and 120 ms at radii 8, 30 and 100. That is about 1.6× faster, and the GPU time includes upload and readback.
- **Default path**: adaptive exposure, details and denoise call `guided::guided_filter_auto`, which tries the GPU and falls back to the CPU if no adapter is found. The fallback has been exercised by the test harness only, not on a machine without a GPU.
- **Still CPU-only**: curves, colour, finishing, local tools, and the Limestone port. Their GPU parity is not started.
- **Tested on one machine**: Apple M4 with Metal. Windows and Linux GPUs are untested.
- Engine tests: 104 passing. render-cli: 1 integration test passing. Desktop crate: type-checks.
- Benchmark tool: `crates/engine/examples/gpu_bench.rs`.

## Eleventh milestone: remaining items from the original spec

Checked against `RUST_TOPAZ_ADJUST5_STYLE_ALL_IN_ONE_CLAUDE_CODE_SPEC.md`.

**Done this round**
- **Functional control matrix** (spec 13): `crates/engine/tests/functional_controls.rs`. Every control has a neutral value that returns the input exactly, and a moderate value that changes pixels. Brightness is checked as a ratio: +1 EV doubles linear luma.
- **New controls** (spec 17): Vignette Curvature, Diffusion Softness (edge-preserving, guided by luma), and tone Positions 1–4 (defaults equal the original anchors, so neutral output is unchanged).
- **Bugs found by the matrix**: (1) Diffusion Softness did nothing, because Finishing's "untouched" early exit didn't include it. Fixed. (2) Details sharpen and boost are gated by the noise estimate, so they do nothing on texture at the image's own noise level. That is the design, and the matrix uses a clear edge or weak step for those controls.
- **EXIF orientation** (spec 11): applied on load in the CLI and desktop app. Test with a generated JPEG tagged orientation 6: output is 4×8 and red sits on top.
- **Cross-platform GPU code**: wgpu uses Metal, Vulkan, DirectX 12 or OpenGL. Added adapter limits (large images stay on the GPU when the device allows), software renderers are skipped, and `PHOTO_ENGINE_GPU=0` forces the CPU path. The engine type-checks for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`. Not yet run on those platforms.

**Still open from the original spec**
- Local tools: edge-aware masks (spec 17 "Edge Aware"), the eraser, and the mask view. The eraser and mask view are UI work.
- Border "Pick from Image" needs UI, since the engine receives the colour as a value.
- I/O: ICC profiles are still ignored, TIFF input and output are untested, and JPEG export with metadata opt-in isn't built.
- Large images: tiled processing with halo margins (A4, section 12) is not built. Full-frame processing of very large files may run out of memory.
- Undo/redo, snapshots and the project schema versioning (A4, section 8) are not in the engine.
- Presets: collections, import/export and per-preset persistence (spec 4.14, section 10).
- Performance: preview pyramid and dirty-node invalidation (section 12).
- Stages 6 to 8 of the staged plan: the finishing UI, batch export, and packaging.
- Photoshop plug-in (A3, A5): excluded by your memory rules.
- Models: TreeNet and HDRNet are downloaded but not wired in. Waiting on your decisions.

Tests: engine 114 (104 unit, 1 CPU-fallback, 9 functional), render-cli 2 integration; 116 in total, all passing.

## Twelfth milestone: remaining items, one by one

**Done**
- **Edge-aware masks** (`local.rs`): a stroke's coverage is weighted by how close each pixel's brightness is to the stroke's core (bilateral similarity). Test: with edge-aware on, a dodge stroke centred on the dark side spills across the edge less than half as much. A first attempt, guided-filtering the mask, did nothing; that's recorded here.
- **Eraser** (`local.rs`, `StrokeKind::Erase`): subtracts coverage from every earlier stroke. A first attempt multiplied instead, which left residue at soft edges; the test caught it.
- **Mask view data** (`local::coverage`): the combined coverage used by the overlay, from the same code `apply` uses. The overlay itself is UI.
- **Border "Pick from Image"** (`finishing::sample_colour`): normalised coordinates, origin top-left, clamped, averaged over a window. The picking UI is still to build.
- **I/O**: TIFF is enabled and round-trips 16-bit exactly (test). JPEG output drops alpha, so JPEG in gives JPEG out (test). Embedded ICC profiles are converted to sRGB on load with `moxcms` (BSD-3/Apache), tested with the system Display P3 profile, which is read at run time and not copied into the repo.
- **Undo/redo and snapshots** (`history.rs`): generic over any cloneable edit state. A drag with one key is one step. Snapshots restore as undoable steps. Five tests.
- **Batch export** (`render-cli`): a folder in gives one output per image, with the same names. Non-images are skipped. Test included.
- **Preview pyramid** (`buffer.rs`, `downscale`): box average in linear light, so each level matches full-size brightness. Desktop still uses one preview size; wiring the levels in is still to do.

**Still open**
- **Local tools UI**: the mask view, eraser and edge-aware controls need the desktop UI.
- **ICC output**: profiles are converted on input, and output is always sRGB without an embedded profile. Embedding the profile is opt-in and not built.
- **JPEG metadata opt-in**: output strips metadata. An opt-in switch is not built.
- **Large images**: tiling with halo margins is not built, and the global statistics for adaptive exposure and colour would need a first pass. This is the biggest remaining risk for very large files.
- **Presets persistence and schema versioning**: the desktop app saves projects with a schema version, but the engine's `Look` has no serialisation or migration yet.
- **Batch export of presets and packaging**: not started.
- **Photoshop plug-in**: excluded by your rules.
- **Models**: TreeNet and HDRNet are downloaded but not wired in, pending your decisions.

Tests: 132 across engine and render-cli, all passing. Desktop type-checks.

## Thirteenth milestone: tiled rendering with halo margins, and install

- **How it works** (`engine/src/tiled.rs`): the image is split into tiles. Each tile is rendered with a halo of neighbouring pixels, sized to cover every chained blur (the sum of the adaptive, details, colour, denoise, softness and bloom halos). Whole-image values are measured first in four streaming passes: the adaptive target, the detail noise level, the colour mean, and the denoise noise level. Every tile then uses those values.
- **Stage changes**: adaptive exposure, colour, details, denoise and finishing take explicit whole-image inputs (`apply_with`, `apply_residue_with`, `apply_region`). Calling them without those inputs behaves as before.
- **Noise estimate**: now a fixed-bin histogram median, so tiles and full frame agree.
- **Bug found and fixed**: the border width was taken from the tile size, not the full image. Border is now correct at tile edges.
- **Verified**: the tiled render matches the full-frame render stage by stage (differences about 1e-6, tolerance 2e-3), and at tile sizes 32, 47 and 150 for the full look. A neutral look is bit-identical.
- **Used by**: the CLI and the desktop export use the tiled path above about 4 MP (tile 1024). Previews stay full-frame.
- **Limits**: strokes, the Limestone style, and "process details independently" render full-frame. With few adaptive regions the halo is large, so tiles approach the full image in size and memory savings shrink.
- **Speed**: a full-featured render of the 9.8 MP Joshua tree photo takes about 31 s, because the tiled path runs five passes. This is correct but slow; optimising it is still open.

**Install (for testing)**
- Desktop app: `/Applications/appsdesktop.app` (release build, tiled export for large images).
- CLI: `~/.local/bin/render-cli` (add `~/.local/bin` to PATH to use it). It takes a file or a folder for batch mode.

Tests: 134 across engine and render-cli, all passing. Desktop type-checks, and the bundle builds without warnings.
