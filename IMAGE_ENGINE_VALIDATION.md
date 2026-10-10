# Image engine validation

## Automated tests written (not yet run)

| Test | File | Checks |
|---|---|---|
| `endpoints_are_fixed` | `crates/engine/src/color.rs` | sRGB transfer maps 0 to 0 and 1 to 1 |
| `every_8bit_value_round_trips_exactly` | `crates/engine/src/buffer.rs` | all 256 grey levels survive decode then encode |
| `rejects_zero_dimensions_and_short_buffers` | `crates/engine/src/buffer.rs` | invalid dimensions and wrong buffer length return errors |
| `non_finite_values_encode_as_zero` | `crates/engine/src/buffer.rs` | NaN and Inf become 0, out-of-range clamps, alpha stays linear |
| `zero_ev_is_identity` | `crates/engine/src/exposure.rs` | 0 EV leaves pixels bit-identical |
| `plus_one_stop_doubles_linear_rgb_and_keeps_alpha` | `crates/engine/src/exposure.rs` | +1 EV doubles linear RGB, alpha unchanged |
| `negative_stop_halves_linear_rgb` | `crates/engine/src/exposure.rs` | -1 EV halves linear RGB |
| `plus_one_stop_matches_independent_f64_formula` | `crates/engine/src/exposure.rs` | 8-bit output within 1 LSB of an independent f64 calculation |
| `rejects_non_finite_ev_and_clamps_large_ev` | `crates/engine/src/exposure.rs` | NaN EV is an error, 1000 EV equals +10 EV |

## Adaptive exposure v1 (crates/engine/src/adaptive_exposure.rs), run 2026-10-10

| Test | Checks |
|---|---|
| `neutral_defaults_are_bit_identical` | default params leave every pixel bit-identical |
| `uniform_image_is_unchanged_at_full_strength` | flat grey stays within 1 LSB |
| `dark_side_lifts_and_bright_side_darkens` | local/global lift has the right sign on a split image |
| `preserves_hue_by_scaling_rgb_together` | R/G ratio and alpha unchanged |
| `highlight_protection_reduces_positive_lift` | protect_highlights shrinks a positive lift |
| `shadow_protection_reduces_negative_lift_in_shadows` | protect_shadows shrinks a negative lift in shadows |
| `rejects_non_finite_and_clamps_regions` | NaN strength is an error; regions 0 clamps to 1 |
| `box_blur_of_constant_is_constant` | blur preserves a constant field |

Status: `cargo test --workspace` PASS (17 engine + 3 FFI).

Real-photo check (2026-10-10, visual only): `render-cli in.png out.png --adaptive 1.0` on `/System/Library/Wallpapers/.default/DefaultAerial.jpg` (4096x2160), crop 900x700 at (x 1600, y 900). Rock and water edges show no visible bright halo or dark ring. Hills and rocks are darker overall. This is one crop, eyeballed only, with no numeric measurement.

Not checked: numeric halo or overshoot measurement, seams (no tiles yet), and any comparison with Topaz output.

## Details v1 (crates/engine/src/details.rs), run 2026-10-10

| Test | Checks |
|---|---|
| `neutral_defaults_are_bit_identical` | default params leave every pixel bit-identical |
| `strength_below_one_softens_a_step_edge` | strength 0 darkens the bright side of a step, versus neutral |
| `halo_guard_bounds_the_change_to_one_stop` | strength 2 with sharpen 2 changes no pixel by more than 1 EV |
| `boost_above_one_enhances_weak_detail` | boost 2 raises a weak step more than boost 0 |
| `higher_threshold_admits_more_detail_into_the_strength_gain` | threshold 1 admits a weak step into strength 0, so it softens; threshold 0 does not |
| `rejects_non_finite_parameter` | NaN radius is an error |

Real-photo check (visual only): `render-cli in.png out.png --details 2.0` on the same aerial photo and crop. Rock outlines and the horizon show no obvious bright or dark ring. One crop, eyeballed, no numbers.

Pipeline (crates/engine/src/pipeline.rs): `independent_mode_matches_dependent_when_details_are_neutral`, `pure_exposure_leaves_independent_detail_unchanged`.

Status: `cargo test --workspace` PASS (25 engine + 3 FFI). Not checked: numeric halo measurement, and any comparison with Topaz.

## FFI (crates/ffi), run 2026-10-10

| Test | Checks |
|---|---|
| `zero_ev_leaves_pixels_and_padding_unchanged` | 0 EV leaves a padded 2x2 buffer identical |
| `positive_ev_changes_pixels_but_never_padding` | +1 EV brightens; row padding stays 0xAA |
| `rejects_bad_arguments_without_touching_buffer` | null, bad dimensions, rowbytes too small and NaN EV return errors and change nothing |
| C harness (`crates/ffi/tests/c/harness.c`) | same behaviours called from C, 10 checks |

Status: `cargo test --workspace` **PASS**, 12 Rust tests. The C harness **PASS**, "all ffi checks passed". One Rust test helper had a bug on its first run (it sliced an 8-byte array as if it held two rows). It was fixed in the test, not the FFI code.

## Commands (once Rust is installed)

```bash
cd "/Users/dibesu/Documents/Reverse Topaz/photo-editor"
cargo test --workspace
cargo run -p render-cli -- input.png output.png --ev 1.0
```

## Status (run 2026-10-10, rustc 1.99.0 stable, aarch64-apple-darwin)

- Compile: **PASS** (`cargo build -p render-cli`, no warnings).
- Tests: **PASS**, 9 of 9 engine tests (`cargo test --workspace`).
- CLI end-to-end: **PASS** on a system PNG (`/System/Library/Desktop Pictures/.wallpapers/Sonoma Horizon/Sonoma Horizon Thumbnail@2x.png`, 214x130):
  - `--ev 0` decoded pixels are identical to the source (max channel diff 0). The PNG bytes differ because the file is re-encoded, so compare pixels, not bytes.
  - `--ev 1.0` raised mean RGB from (80.5, 78.3, 62.2) to (108.9, 106.6, 86.2).
  - `--ev -1.0` lowered it to (56.7, 55.1, 43.0).
- The pixel check used a throwaway Python PNG decoder written for the check. It is not part of the repo.
- Golden-image suite: not started. There are no licensed fixture images yet.
- CPU/GPU parity: not applicable yet.

## Known gaps in validation

- Only global exposure is covered. No adaptive, no regions, no tiles, no halo tests.
- No performance measurements.
- No 16-bit or ICC tests.

## Combined CLI look v1 (2026-10-10, visual only)

Command: `render-cli in.png out.png --ev 0.2 --adaptive 0.6 --details 1.3 --saturation 1.15` on the aerial photo (4096x2160), crop 900x700 at (x 1600, y 900). Warmer, crisper rock detail, no obvious halo. Eyeballed only. This is our own look, not a match for Topaz Adjust 5.

## Engine modules, run 2026-10-10

`cargo test --workspace`: 56 engine tests and 3 FFI tests pass, no warnings. New module test groups:

- Colour (`color_adjust.rs`, 7 tests): OKLab round-trip, neutral identity, zero saturation removes chroma, saturation raises chroma, hue wrap and chroma preservation, boost favours low chroma, NaN rejection.
- Finishing (`finishing.rs`, 10 tests): neutral identity, warmth direction, full transparency returns the original, vignette darkens corners only, grain is deterministic, border colours the edge only, diffusion spreads from a highlight, white tone is neutral, NaN rejection.
- Curve (`curve.rs`, 5 tests): identity, midpoint lift, monotone with no overshoot, hue preservation, bad input rejected.
- Noise (`noise.rs`, 3 tests): zero amount identity, small fluctuations reduced, strong edge kept.
- Local (`local.rs`, 6 tests): no strokes identity, dodge and burn direction inside the mask, zero strength identity, Brush Out returns toward the original, NaN rejection.
- Look (`pipeline.rs`): default look is bit-identical.

Real-photo check: the combined CLI command (`--ev 0.2 --adaptive 0.6 --details 1.3 --saturation 1.15 --warmth 0.3 --vignette 0.4 --grain 0.2`) on the aerial crop looks warmer with subtle grain. Eyeballed only.

## Basic exposure and colour regions (2026-10-10)

Basic exposure tests: neutral identity, contrast spreads tones, black boost lifts shadows only, NaN rejection. Engine total: 60 tests. Workspace total with the desktop backend: 60 engine, 4 desktop backend, 3 FFI, all passing.

## Performance, CPU (2026-10-10)

Method: release build (`cargo build --release -p render-cli`), wall clock around one run, on the 4096x2160 aerial photo (8.8 MP). Machine: this Mac (Apple silicon, 10 cores). Single runs, so expect noise of about 10 to 20 percent.

| Run | Before Rayon | After Rayon (box blur, colour, exposure, exact sRGB LUT decode) |
|---|---|---|
| all stages on | 2.81 s | 1.58 s |
| look only, stage timer | look 2.0 s, decode 0.08 s, encode 0.04 s, save 0.06 s | not re-timed |

Benchmark command: `time ./target/release/render-cli in.png out.png --ev 0.2 --adaptive 0.6 --details 1.3 --saturation 1.15 --warmth 0.3 --vignette 0.4 --grain 0.2`

Not measured yet: the 960 px preview inside the desktop app (expected about a tenth of the full-size time, unmeasured), memory peak, tile-based rendering, and GPU. The spec's targets (for example 200 ms preview at 24 MP) are goals, not results.
