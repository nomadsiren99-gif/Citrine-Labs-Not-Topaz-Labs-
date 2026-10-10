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

## D7 — Documentation files at project root (2026-10-10)
The brief lists eight continuity files. They sit at the root of `photo-editor/`, not in `docs/` as the spec's tree suggests.
