#!/bin/bash
# Compiles and runs the host simulation against RustExposure.cpp and the release engine-ffi library.
# Requires the same PS_SDK_DIR as build.sh. Run build.sh first so target/release exists.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PLUGIN="$(cd "$HERE/.." && pwd)"
ROOT="$(cd "$PLUGIN/../.." && pwd)"
SDK="${PS_SDK_DIR:?set PS_SDK_DIR to the pluginsdk directory of the Adobe SDK}"
BIN="$ROOT/target/host_sim"

clang++ -std=c++17 -arch arm64 -mmacosx-version-min=12.0 -fvisibility=default -O1 \
  -I "$SDK/photoshopapi/photoshop" \
  -I "$SDK/photoshopapi/pica_sp" \
  -I "$SDK/photoshopapi/resources" \
  -I "$SDK/samplecode/common/includes" \
  -framework CoreFoundation \
  "$HERE/host_sim.cpp" "$PLUGIN/src/RustExposure.cpp" \
  "$ROOT/target/release/libengine_ffi.a" \
  -o "$BIN"
"$BIN"
