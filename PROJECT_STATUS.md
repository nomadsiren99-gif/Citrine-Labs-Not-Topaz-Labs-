# Project status

Last updated: 2026-10-10

## Repository audit (Milestone 0)

- Location: `/Users/dibesu/Documents/Reverse Topaz/photo-editor/` (new, sibling of the reference folder, not inside it).
- Before this session the folder contained only the spec `.md` and the reference folder `Topaz Adjust AI 1.0.5 [FileCR]/` plus its `.zip`. No prior Rust, C++ or Photoshop code existed, and there was no git repository.
- Toolchains on this Mac:
  - Rust (`cargo`, `rustc`): **not installed**. Nothing in `~/.cargo` or `~/.rustup`, no Homebrew.
  - Xcode developer dir present, Apple clang 21.0.0.
  - Node at `~/.local/bin/node`.
- Host: **Adobe Photoshop 2026** found in `/Applications`. Version 27.11.0 (from its `Info.plist`). Adobe C++ SDK: **not present** in the project or the spec folder.

## Completed

- Workspace skeleton: `Cargo.toml` (members `engine`, `render-cli`), `.gitignore`.
- `crates/engine`: sRGB transfer functions, `LinearImage` (f32 linear RGBA, straight alpha), 8-bit sRGB import/export, global exposure in EV.
- `crates/render-cli`: `render-cli <in> <out> [--ev N]`, loads PNG/JPEG, applies exposure, saves.
- Unit tests written (see `IMAGE_ENGINE_VALIDATION.md`).

## Verified (2026-10-10)

- Rust stable 1.99.0 installed via rustup (`~/.cargo`, `~/.rustup`).
- `cargo test --workspace`: 9 of 9 pass. `cargo build` has no warnings.
- CLI exposure check: 0 EV matches source pixels exactly, +1 and −1 EV brighten and darken as expected. Details are in `IMAGE_ENGINE_VALIDATION.md`.

## Build status (2026-10-10)

The build is not finished. Done: engine modules (adaptive exposure, details, colour, noise, curve, local strokes, finishing, pipeline with `Look`), CLI, and a desktop app that renders previews and exports. Performance: full-size 8.8 MP render takes about 1.6 s on this Mac, with Rayon (see IMAGE_ENGINE_VALIDATION.md). Still to do: tile-based rendering, GPU, memory caps, zoom and pan, histogram, undo and redo, snapshots,  batch export, colour regions, high-quality denoise, a brush UI with persistent masks, tiling and GPU, performance diagnostics, our own preset library, and packaging. The Photoshop bridge is paused at your request.

## Not verified

- Golden-image tests against fixture photos (none yet), ICC, 16-bit, performance.
- Photoshop Smart Filter behaviour (see Blocked).
- Adobe SDK: the user-supplied SDK zip was extracted to scratch. The Dissolve sample builds on this Mac after two changes in the scratch copy only. The PiPL is in the bundle as `PiPLs.json`, which the SDK docs say is the correct modern form. The bundle has not been loaded in Photoshop yet.
- Desktop app: `apps/desktop` is a Tauri 2 + React/TypeScript app. The Rust backend (`src-tauri`) calls the engine for `render_preview` (960 px proxy) and `export_image` (full size). The UI has presets (our own), a before/after hold, 11 sliders, undo/redo, zoom (fit, 100%, 200%), a live luminance histogram, and preview and export. Projects save and open as a versioned JSON file (schemaVersion 1: source path, every control, brush strokes, model). Images open through an Open button, drag and drop, or a typed path. Save goes through a native save dialog. `npm run build` and `cargo test --workspace` pass. `npm run tauri dev` starts and stays running. I have not checked the window by hand.
- Rust C ABI: `engine-ffi` exposes the exposure kernel with row-padding support. It passes 3 Rust tests and a 10-check C harness.
- Photoshop native shim: `plugins/photoshop-native/` builds `RustExposure.plugin` (arm64) against the supplied SDK. A host simulation passes 8 of 8 checks. **It has not been installed or run in Photoshop.** It has a fixed EV and no persistence yet.

## Blocked

- Photoshop Smart Filter proof (Gate 0): blocked on the Adobe C++ SDK download and on confirming the Photoshop 2026 build. See `PHOTOSHOP_INTEGRATION.md`.
- Reference executable identity: static check only. Not run, not verified.

## Next

See `NEXT_TASKS.md`.
