# Architecture

## Current layout

```
photo-editor/
  Cargo.toml                 workspace (resolver 2)
  crates/
    engine/                  pure CPU reference engine, no UI, no host code
      src/color.rs           sRGB <-> linear transfer functions
      src/buffer.rs          LinearImage: f32 RGBA, linear light, straight alpha
      src/exposure.rs        global exposure, gain = 2^EV, clamped to +/-10 EV
    render-cli/              thin CLI over engine; PNG/JPEG in and out
```

Planned, not created yet: `desktop/` (Tauri 2 + React), `photoshop-native/` (C++ shim), `engine-ffi` (C ABI), GPU (`wgpu`), presets and project format.

## Invariants

- Pixel math runs on linear light in f32. The sRGB curve is applied only at decode and encode.
- Alpha is straight, stored linearly, and is never transfer-encoded or changed by colour operators.
- Non-finite parameters are rejected. Non-finite pixels encode as 0. Out-of-range values are clamped.
- One kernel implementation serves the CLI, the desktop app and the Photoshop bridge. Previews may later use documented scale shortcuts, but must not fork the maths.

## Planned dependencies

- `image` 0.25 (codecs, CLI only so far). The engine crate has no dependencies.
- `rayon` for CPU parallelism once the kernels exist. Not added yet.
- `wgpu` for GPU kernels after the CPU reference is validated.
