# Decisions

## D1 — Project location (2026-10-10)
Put the code in `photo-editor/`, next to the spec, not inside the reference folder. The reference folder holds an unknown executable and must stay separate.

## D2 — Working colour space (2026-10-10)
Linear light, sRGB primaries, f32, straight alpha. The sRGB transfer is applied only at decode and encode. Reason: the spec forbids gamma-space maths. A wide-gamut working space is deferred until ICC work starts.

## D3 — Codec scope for M1 (2026-10-10)
8-bit sRGB PNG and JPEG only. ICC profiles are ignored and 16-bit input is reduced to 8-bit. Reason: keep the first slice small. Both are logged in `FEATURE_MATRIX.md`.

## D4 — Exposure range (2026-10-10)
Clamped to +/-10 EV, NaN and Inf rejected. The range is a proposed engineering limit, not a verified Adjust 5 value.

## D5 — Reference executable not run (2026-10-10)
Only static checks. Running an unknown binary needs an isolated Windows VM, and the bundled instructions describe a licence bypass, which is not followed.

## D6 — No git repository yet (2026-10-10)
Not initialised. The spec asks for small commits, but commits happen only when the user asks. Offer `git init` in the next session.

## D8 — Adaptive exposure v1 mappings (2026-10-10)
Box-blur local mean of log2 luminance with clamp-to-edge borders, radius = longest side / regions / 2. Lift = (global mean - local mean), clamped to +/-3 EV, times strength and (1 + contrast). Protections act on the sign of the lift: highlights limit positive lift, shadows limit negative lift. All mappings are proposed. Reason: smallest kernel that meets the spec's directional tests.

## D7 — Documentation files at project root (2026-10-10)
The brief lists eight continuity files. They sit at the root of `photo-editor/`, not in `docs/` as the spec's tree suggests.

## D9 — Guided base from the author's Limestone (2026-10-10)
The guided filter (He et al. 2010), ported from the author's Limestone repo, now sets the base in adaptive exposure (edge-aware local mean), Details (base so that edges stay out of the detail residue, eps 0.02 in log2 units) and noise suppression (eps 0.03). Consequence: Details now targets fine texture, not step edges, so the Details tests were rewritten around texture. Reason: halo reduction and the author's own design. Other Limestone stages (gamma tone mapping, smoothing and sharpening weight curves, spline curves, colour and finishing) are not ported yet.

## D10 — Limestone stage set and factory presets (2026-10-10)
All of Limestone's stages are ported into `crates/engine/src/limestone.rs` and selected with `Style::Limestone` (the Model switch in the UI). The default engine is unchanged. The 21 Limestone factory presets are kept, renamed to our own names (for example "Spicify" becomes "Vivid Punch"), and "Topaz" is removed from their descriptions. The values are converted to our slider convention in `apps/desktop/src/presets.ts`. Presets switch the model on.

Risk to note: Limestone describes its presets as reverse-engineered from Topaz Adjust 4. Renaming them does not remove that origin. Before any public release, decide whether these values can be used.
