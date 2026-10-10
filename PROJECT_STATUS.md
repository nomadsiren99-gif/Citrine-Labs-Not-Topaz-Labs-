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

## Not verified

- Golden-image tests against fixture photos (none yet), ICC, 16-bit, performance.
- Photoshop Smart Filter behaviour (see Blocked).

## Blocked

- Photoshop Smart Filter proof (Gate 0): blocked on the Adobe C++ SDK download and on confirming the Photoshop 2026 build. See `PHOTOSHOP_INTEGRATION.md`.
- Reference executable identity: static check only. Not run, not verified. See `REFERENCE_ANALYSIS.md`.

## Next

See `NEXT_TASKS.md`.
