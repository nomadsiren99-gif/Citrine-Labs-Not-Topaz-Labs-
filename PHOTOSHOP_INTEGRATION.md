# Photoshop integration

## Status: BLOCKED (feasibility not yet proven)

No native Smart Filter exists. Nothing in this project claims one. A UXP panel, a duplicated layer, or an externally rendered image is **not** a Smart Filter and is not acceptable as one.

## Known facts

- Host: **Adobe Photoshop 2026**, version **27.11.0** (`CFBundleShortVersionString` and `CFBundleVersion` in its `Info.plist`). SDK compatibility with this build is not yet checked.
- SDK: the Adobe Photoshop C++ SDK is **not** present in this project or the spec folder. It must be downloaded from Adobe by the user or with the user's permission. Do not assume its licence or layout.
- Toolchain: Xcode with Apple clang 21 is available for a macOS C++ shim. Rust is not installed.

## Feasibility gate (M0) checklist

1. Photoshop version read: 27.11.0. Done.
2. Obtain the C++ SDK. Record its version and the path to the filter examples.
3. Build the smallest native filter with the SDK's example structure. It applies one exposure multiplier.
4. Confirm the Smart Filter support flag in the PiPL. Check the current SDK headers. Do not copy an old declaration.
5. Convert a test raster layer to a Smart Object, apply the filter, and confirm a named Smart Filter appears.
6. Save the PSD, close, reopen, double-click the filter, change one value, and confirm the output changes.
7. Confirm Cancel leaves the document unchanged.

Until steps 1 to 7 pass on the installed Photoshop, the M0 gate stays **BLOCKED**.

## Architecture constraints (from the spec)

- The native plugin is a thin C++ shim. Image maths lives in Rust behind a C ABI.
- Settings are stored in the PSD through the SDK's parameter descriptors. An external cache file must never be the only copy.
- Rendering must work headless, without the desktop UI running.
- The local IPC must use a per-session token and a loopback or Unix socket. Nothing is exposed on a public port.
