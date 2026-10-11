# Next tasks

Ordered. Stop at the first item that is blocked and report it.

0. Done (v1 look): `render-cli` chains exposure, adaptive, details, colour (see IMAGE_ENGINE_VALIDATION.md). Local adjustments, finishing modules and a UI are still to do.
0a. **Blocked on a decision:** Quartz (github.com/anuzdhk/Quartz) reviewed, not imported. No LICENSE file (GitHub shows none, README badge says MIT). Its presets name and copy Topaz Adjust 4 looks, which the spec forbids. It commits two prebuilt Windows .exe files of unknown origin, and the README clone URL points at another repo. Options: (a) owner confirms a licence and we use it with attribution, minus the Topaz presets; (b) reference only, reimplement ideas (its Process Details Independently and region mapping are useful); (c) skip. Recommended: (b).

1. Done: Rust installed, `cargo test --workspace` passes 9/9, CLI exposure check recorded.
2. Test JPEG input/output through the CLI, then decide whether ICC and 16-bit input belong in M1 or later.
3. Add `rayon` only if a kernel needs it, and add a benchmark before adding it.
4. Done: Adaptive Exposure v1 (8 tests). Next: check halos and seams on a real photo through the CLI, and add the `--adaptive` flag. Original task text follows.
   **Adaptive Exposure v1** in `crates/engine/src/adaptive_exposure.rs`: log2 luminance, a guided or box-filter local base, the protections and Regions. Tests: identity at 0, no tile seams, no halo on a step edge.
5. Done: Details v1 (6 tests, direction tests for every control). Real-photo halo check done (one crop, visual). Open: numeric halo measurement, Topaz comparison (needs a lawful reference copy). Process Details Independently done (pipeline.rs).

   Next, per the spec: own desktop UI (Tauri 2 + React/TypeScript) on top of the engine. Topaz Photo AI: public docs and guides only. No binary analysis of the product.
6. Photoshop M0, in progress:
   - Done: the SDK's PiPL is in the bundle as `PiPLs.json`, so no compile step is needed on Mac. Rust FFI `pe_apply_exposure_rgba8` is built and tested from Rust and C.
   - Done: C++ shim `plugins/photoshop-native/` builds against the SDK at `PS_SDK_DIR` (the SDK is not copied into the repo). Host simulation: 8 of 8 checks pass.
   - Hang cause still open (see PHOTOSHOP_INTEGRATION.md, diagnostic report). Plug-in stays out of Photoshop until a test isolates it.
   - Next, needs the user's go-ahead: install `build/RustExposure.plugin` into a Photoshop 27 plug-ins folder (the exact path must be confirmed for Photoshop 27.11), start Photoshop, and check that "Rust Exposure..." appears under Filter for an RGB 8-bit Smart Object. Record the result in `PHOTOSHOP_INTEGRATION.md`.
   - Then: a parameter dialog or descriptor storage so the EV persists and can be re-edited on double-click, then PSD save and reopen.
   - Do not claim success until the PSD round trip passes.
7. Decide with the user whether to run `git init` and make the first commit.
8. Reference: if a lawfully obtained copy of the newer Topaz product is provided, run it only in an isolated Windows VM and record the About dialog version.

- Done: project save and open (versioned JSON, schemaVersion 1). Next: a tested migration path for future schema versions, and tile-based rendering.

- Done: exact Quartz port (D12), Quartz mode in the app with exact ranges, presets, a points-based curve editor and an RGB and luminance histogram with channel toggles.
- Open: Ours mode still uses the three-slider curve. Undo covers both modes. Snapshots are not in the new UI yet.
- Open: check the exact pipeline in the window (not yet tested by hand); check the curve interaction against Lightroom/Photoshop feel.
