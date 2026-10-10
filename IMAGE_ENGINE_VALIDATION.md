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
