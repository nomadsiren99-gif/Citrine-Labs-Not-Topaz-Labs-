# Photoshop integration

## Status: BLOCKED (feasibility not yet proven)

No native Smart Filter exists. Nothing in this project claims one. A UXP panel, a duplicated layer, or an externally rendered image is **not** a Smart Filter and is not acceptable as one.

## Known facts

- Host: **Adobe Photoshop 2026**, version **27.11.0** (`CFBundleShortVersionString` and `CFBundleVersion` in its `Info.plist`).
- Toolchain: Xcode 27.0 (build 27A266a), SDK `macosx27.0`, arm64. Rust 1.99.0 is installed.

## SDK received (2026-10-10)

Source: user-supplied `adobe_photoshop_sdk_2026_mac_v2.zip` (21 MB, 3,943 entries). It was extracted into a scratch directory, not into the repo. The zip's file timestamps are 2025-12-18.

- Layout: `photoshopsdk/pluginsdk/` (plug-in SDK), `photoshopsdk/connectionsdk/` (a separate remote-control SDK, not used for filters), `samplecode/filter/` (filter samples: colormunger, dissolve, hidden, mfcplugin, poormanstypetool, propetizer).
- **SDK version is not stated** in the docs. The `ReadMe.html` and `documentation.html` only carry older strings (for example "Photoshop Version 19.1, January 2018"). The Dissolve sample's `Info.plist` reports `CFBundleShortVersionString` 24.5.0 and copyright 2023. So the samples are from a 24.x-era sample set. **Whether the headers match Photoshop 27.11 is unverified.**
- Smart Filter flag: `FilterLayerSupport` in `PiPLs.json`, key `flly` (`PIFilterLayerSupport` in `photoshopapi/photoshop/PIGeneral.h`). The Dissolve sample sets it to `true`. The PiPL schema is in `pluginsdk/pipl-schema.json`.
- Parameter storage: the samples use the descriptor API (`descriptorParameters`, `readDescriptorProcs`, `writeDescriptorProcs`) and the `DissolveScripting.cpp` helpers. The Smart Filter re-edit path has not been read yet.

### Build attempt (scratch copy, not installed)

- `Dissolve` (Mac, Xcode project) does not build as shipped:
  1. `MACOSX_DEPLOYMENT_TARGET` is 10.13. This Xcode needs 12.0 or later.
  2. `DissolveProxyView.m:304` uses deprecated `graphicsPort`, and deprecation is an error by default.
- With `MACOSX_DEPLOYMENT_TARGET=12.0` and `-Wno-error=deprecated-declarations`, the build **succeeds**. Output is `Dissolve.plugin`, arm64, `LSMinimumSystemVersion` 12.0.
- **PiPL resolved (correction to the previous note):** the SDK's `documentation/html/PGPiPLResources.html` says modern Mac plug-ins carry `Contents/Resources/PiPLs.json` inside the bundle, and no binary PiPL resource is needed. The built `Dissolve.plugin` does contain that file. My earlier worry about a missing resource fork was wrong. The Windows path uses a `JSON_PIPL` resource instead, and `samplecode/resources/Cnvtpipl.exe` is only for Windows.
- Nothing has been copied into the Photoshop plug-ins folder and Photoshop has not been run with the plugin. That needs the user's permission. `samplecode/misc/SDKInstall.sh` copies to `/Library/Application Support/Adobe/Plug-Ins/CC/SDK`, a system path.

## Native shim (built, host-simulated, NOT run in Photoshop)

- `plugins/photoshop-native/` contains `PiPLs.json` (Filter, `FilterLayerSupport` true, RGB 8-bit only), `Info.plist` (type `8BFM`, signature `8BIM`), `src/RustExposure.cpp` and `build.sh`.
- `PS_SDK_DIR=<pluginsdk> ./build.sh` builds `build/RustExposure.plugin`. Verified on this Mac: arm64, `PluginMain` and `pe_apply_exposure_rgba8` exported, plist and PiPL JSON valid.
- The shim tiles with `outTileWidth`/`outTileHeight`, uses `inRect`/`outRect` and `advanceState`, gathers RGB(A) into RGBA8, calls the FFI with EV = 1.0, and writes RGB back. Alpha is written back only if the host gives 4 planes.
- `plugins/photoshop-native/tests/run_host_sim.sh` drives `PluginMain(filterSelectorStart)` with a hand-built `FilterRecord`. **8 of 8 checks pass**: tiled output equals full-frame output (4 planes, with alpha), host row padding is untouched, 3-plane RGB matches the reference, and 16-bit input gets `filterBadMode` with no tile requested. This tests the shim's own logic only. It proves nothing about Photoshop.
- **Not done:** no parameter dialog (EV fixed at 1.0), no descriptor persistence, so the Smart Filter cannot be re-edited yet. The `EnableInfo` expression, the `FilterCaseInfo` values (copied from the Dissolve sample) and the plane order are unverified against a running host.
### Diagnostic report (2026-10-10, read-only check)

- macOS report `/Library/Logs/DiagnosticReports/Adobe Photoshop 2026_2026-10-10-224511_..._cpu_resource.diag`: Photoshop 27.11.0, PID 33314, 90 s CPU over 128 s, limit exceeded.
- Sampled stacks are in Photoshop's main loop (`NSApplication run`) and AppKit. **No `RustExposure` frames and no plug-in loading frames.** The report does not show that the plug-in caused the hang, and it does not rule it out.
- Decision: keep the plug-in out of the plug-ins folders until a test shows the cause. Do not reinstall without the user's go-ahead.

### Hang finding (2026-10-10, newest)

- After `RustExposure.plugin` was installed in the system folder, Photoshop 27.11 hung: about 12 minutes at 99.8% CPU, not answering AppleEvents. Its log showed no plug-in error.
- The hung process was stopped with SIGTERM. A second copy was stopped the same way (SIGKILL only when needed). Both had no open documents.
- The plug-in was removed with sudo. Photoshop was relaunched without it and responds, and its CPU settled to about 9% after two minutes.
- **Likely cause: the plug-in** (unsigned by Adobe's team, or a PiPL/startup problem). Not proven. Do not reinstall it until the cause is found.
- The Filter menu could not be read: the menu bar access failed while Photoshop was hung. Accessibility is now granted for the Terminal tool.

### Current state (2026-10-10, earlier)

- Installed to the documented folder `/Library/Application Support/Adobe/Plug-Ins/CC/RustExposure.plugin` (copied with sudo in the user's Terminal; the user entered the password). The per-user copy was removed.
- Photoshop stopped answering AppleEvents after the first relaunch, and was then stopped with SIGTERM. Its open-document count was unknown at that moment (the last successful check showed 0). Relaunched; it now responds.
- The relaunched Photoshop does not map `RustExposure` (`lsof`). Plug-ins may load lazily, so this is not conclusive.
- The Filter menu cannot be read: osascript has no Accessibility permission on this Mac.
- Still not verified: whether the plug-in loads, whether Photoshop accepts the ad-hoc signature, and the Smart Filter round trip.

### Install and load attempt (2026-10-10, earlier)

- Bundle re-signed ad hoc (`codesign --force --deep --sign -`). `codesign --verify` passes, and the Info.plist and resources are sealed. Photoshop's own team ID is `JQ525L2MZD`. Our plug-in is not signed by that team, so Photoshop's hardened runtime may refuse to load it. Untested.
- Installed to `~/Library/Application Support/Adobe/Plug-Ins/CC/RustExposure.plugin`. This path is **not documented** in the SDK. The SDK documents only the app's `Required/Plug-Ins`, the app's `Plug-ins`, and `/Library/Application Support/Adobe/Plug-Ins/CC`. The last two are root-owned here, and sudo needs a password.
- Photoshop 2026 (27.11) was quit (0 open documents) and relaunched. Its logs do not mention the plug-in. `lsof` shows it mapped the system Camera Raw plug-in, but not `RustExposure`.
- **Not verified:** whether "Rust Exposure..." appears under Filter. Reading the menu through System Events needs Accessibility permission for osascript, which is denied on this Mac.
- To install in the documented system folder (needs your password):

```bash
sudo cp -R "/Users/dibesu/Documents/Reverse Topaz/photo-editor/plugins/photoshop-native/build/RustExposure.plugin" "/Library/Application Support/Adobe/Plug-Ins/CC/"
```

- Then restart Photoshop and open Filter. "Rust Exposure..." should appear if the plug-in loads. If it does not, check whether Photoshop rejected the ad-hoc signature (hardened runtime / library validation).
- **Not done:** parameter dialog and descriptor persistence, so the Smart Filter cannot be re-edited yet.

## Rust-side ABI (done, tested without Photoshop)

- `crates/ffi` (`engine-ffi`) exports `pe_apply_exposure_rgba8(pixels, width, height, row_bytes, ev)`, returning 0 on success or a negative code (-1 null, -2 bad dimensions, -3 rowbytes too small, -4 bad parameter, -5 panic).
- It honours host row padding (`row_bytes`), leaves padding untouched, and leaves the buffer unchanged on error.
- It is tested in Rust (3 tests) and from C with `crates/ffi/tests/c/harness.c` (10 checks, all pass).
- Host channel order: `PIFormat.h` (~line 1009) says Photoshop stores RGB with alpha as RGBA, and that frame buffers often use ARGB. The plug-in changes the order with `planeMap`. Its default is the linear map `planeMap[i]=i`, which already matches our RGBA8 layout. The ARGB example (`planeMap[0]=3, [1]=0, [2]=1, [3]=2`) is only needed if the plug-in works in ARGB. The shim must also use the buffer's rowBytes as the stride. Neither is written yet. Both must be checked against a real host run.

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
