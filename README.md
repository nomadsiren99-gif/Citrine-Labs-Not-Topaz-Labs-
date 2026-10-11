# Citrine Labs (Not Topaz Labs)

Citrine is a photo-enhancement engine and desktop app written in Rust and TypeScript. It applies exposure, local contrast, detail, colour, noise and curve adjustments in linear light, with a Tauri 2 and React interface.

Citrine Labs is not affiliated with, endorsed by, or connected to Topaz Labs or any of its products.

## Repository layout

```
Cargo.toml                  Rust workspace
crates/
  engine/                   CPU engine: exposure, adaptive exposure, detail,
                            colour, noise, curve, local strokes, finishing,
                            pipeline with presets, Rayon parallelism
  quartz-core/              second processing model, with its own parameters,
                            presets and tests
  ffi/                      C ABI (static and dynamic library) and a C harness
  render-cli/               command-line renderer: PNG or JPEG in and out
apps/desktop/               Tauri 2 and React desktop app
  src/                      interface: sliders, curves, histogram, presets
  src-tauri/                Rust backend that calls the engine
plugins/photoshop-native/   Photoshop filter shim (not yet run in Photoshop)
.github/workflows/build.yml CI build for macOS, Windows and Linux
```

## Requirements

- Rust stable, installed with rustup
- Node.js 22 or later, for the desktop front end

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
cargo run -p render-cli -- input.png output.png --ev 0.5
```

## Desktop builds

The workflow in `.github/workflows/build.yml` builds the desktop app on macOS, Windows and Linux. Start it from the Actions tab with **Run workflow**, or push a tag starting with `v`. Each platform's bundles are uploaded as a workflow artifact.

## How the pipeline works

- Pixel maths runs in linear light, in 32-bit float with straight alpha. The sRGB transfer function is applied only when decoding and encoding.
- Non-finite parameters are rejected, and out-of-range values are clamped.
- The command-line renderer, the desktop app and the Photoshop shim share one kernel implementation.
- Projects are saved as versioned JSON files. Each file records the source path, every control, brush strokes and the selected model.
- The desktop app renders a reduced-size preview and exports at full size.

## Licensing

- The bundled denoise model `crates/engine/models/treenet_denoise_super_light.onnx` comes from the RawForge project and is licensed under MIT. The licence text is in `crates/engine/models/LICENSE-RawForge-MIT.txt`.
- The `quality/` folder of local comparison renders is not part of this repository.
