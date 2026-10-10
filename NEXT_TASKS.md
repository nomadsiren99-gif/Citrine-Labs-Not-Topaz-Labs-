# Next tasks

Ordered. Stop at the first item that is blocked and report it.

1. Done: Rust installed, `cargo test --workspace` passes 9/9, CLI exposure check recorded.
2. Test JPEG input/output through the CLI, then decide whether ICC and 16-bit input belong in M1 or later.
3. Add `rayon` only if a kernel needs it, and add a benchmark before adding it.
4. **Adaptive Exposure v1** in `crates/engine/src/adaptive_exposure.rs`: log2 luminance, a guided or box-filter local base, the protections and Regions. Tests: identity at 0, no tile seams, no halo on a step edge.
5. Details (Strength, Boost, Threshold, Radius, Sharpen) with a neutral-value test.
6. Photoshop M0: ask the user for the Adobe C++ SDK (or confirm where it is), read the Photoshop 2026 version, then build the minimal native filter. Do not claim success until the PSD round trip passes.
7. Decide with the user whether to run `git init` and make the first commit.
8. Reference: if a lawfully obtained copy of the newer Topaz product is provided, run it only in an isolated Windows VM and record the About dialog version.
