# Citrine Labs (Not Topaz Labs)

A photo-enhancement engine and desktop app written in Rust and TypeScript.
It applies exposure, local contrast, detail, colour, noise and curve adjustments
in linear light, with a Tauri 2 + React interface on top.

> **Disclaimer.** This project is **not** affiliated with, endorsed by, or derived
> from Topaz Labs or its product Topaz Adjust AI. It is an independent
> implementation. The "Quartz" model and presets are ported from the author's
> own Quartz repository (see [Licensing and provenance](#licensing-and-provenance)).

## Repository layout

```
Cargo.toml                 Rust workspace (resolver 2)
crates/
  engine/                  CPU reference engine: exposure, adaptive exposure,
                           details, colour, noise, curve, local strokes,
                           finishing, pipeline with Look presets, Rayon parallelism
  quartz-core/          "adjust-core" crate: the Quartz model, copied from
                           the author's repository with its parameters, presets
                           and pixel handling unchanged
  ffi/                     engine-ffi: C ABI (staticlib + cdylib) and a C harness
  render-cli/              command-line renderer: PNG/JPEG in, PNG/JPEG out
apps/desktop/              Tauri 2 + React/TypeScript desktop app
  src/                     UI: sliders, curves, histogram, presets, undo/redo, zoom
  src-tauri/               Rust backend calling the engine (preview and export)
plugins/photoshop-native/  C++ Photoshop filter shim (not yet run in Photoshop)
ARCHITECTURE.md            layout and invariants
DECISIONS.md               numbered design decisions (D1 to D12)
FEATURE_MATRIX.md          what is implemented, partial or planned
IMAGE_ENGINE_VALIDATION.md validation notes and measurements
NEXT_TASKS.md              prioritised backlog
PROJECT_STATUS.md          status log
```

## Requirements

- Rust stable (tested with 1.99.0), installed through rustup
- Node.js and npm, for the desktop front end
- macOS is the platform used so far; the Rust crates should build elsewhere but
  this has not been checked

## Build and test

```bash
cargo check --workspace
cargo test --workspace
```

Desktop app:

```bash
cd apps/desktop
npm install
npm run build
npm run tauri dev
```

Command-line renderer:

```bash
cargo run -p render-cli -- <in.png> <out.png> --ev 0.5
```

## How the pipeline works

- Pixel maths runs in **linear light, f32, straight alpha**. The sRGB transfer
  function is applied only when decoding and encoding (D2).
- Non-finite parameters are rejected; out-of-range values are clamped.
- One kernel implementation serves the CLI, the desktop app and the planned
  Photoshop bridge, so the maths is not forked.
- Parameters are stored in a versioned JSON project file (`schemaVersion` 1),
  which records the source path, every control, brush strokes and the model.
- The desktop app renders a 960 px proxy for preview and the full-size image for
  export.

Two models are available in the app:

- **Ours**: the engine in `crates/engine`, with its own sliders and presets.
- **Quartz**: the exact port in `crates/quartz-core`, with its own ranges
  and factory presets.

## Status

Working: the engine modules listed above, the CLI, the desktop app (preview,
export, presets, before/after hold, undo/redo, zoom, histogram, project save and
open), the C ABI with its tests, and CPU parallelism.

Not done: tile-based rendering, GPU kernels (`wgpu`), memory caps, batch export,
colour regions, a brush UI with persistent masks, ICC and 16-bit input, and
packaging. The Photoshop shim builds but has **not been installed or run in
Photoshop**.

Measured: a 8.8 MP image takes about 1.6 s end to end on the development Mac
after the Rayon work (see `IMAGE_ENGINE_VALIDATION.md`).

Not yet verified: golden-image tests against fixture photos, ICC handling,
16-bit input, and the desktop window checked by hand.

## Licensing and provenance

- **Not Topaz Labs.** No code, model or asset is taken from Topaz Labs or Topaz
  Adjust AI. The reference folder used during the project is kept separate and
  its executable was never run (D1, D5).
- **Bundled model.** `crates/engine/models/treenet_denoise_super_light.onnx` comes
  from the RawForge project under the MIT licence. Its provenance and checksum are
  in [`crates/engine/models/README.md`](crates/engine/models/README.md).
- **Quality outputs.** Large comparison renders under `quality/` are not committed
  (see `.gitignore`).

## Contributing

Small, reviewed commits. Record any decision that changes behaviour in
`DECISIONS.md`, and update `FEATURE_MATRIX.md` and `PROJECT_STATUS.md` in the same
change.
