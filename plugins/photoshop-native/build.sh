#!/bin/bash
# Builds RustExposure.plugin (macOS, arm64) against an Adobe Photoshop SDK you supply.
# The SDK is not copied into this repo. Usage:
#   PS_SDK_DIR="/path/to/photoshopsdk/pluginsdk" ./build.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SDK="${PS_SDK_DIR:?set PS_SDK_DIR to the pluginsdk directory of the Adobe SDK}"
OUT="$HERE/build/RustExposure.plugin"

cargo build -p engine-ffi --release --manifest-path "$ROOT/Cargo.toml"

rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cp "$HERE/Info.plist" "$OUT/Contents/Info.plist"
cp "$HERE/PiPLs.json" "$OUT/Contents/Resources/PiPLs.json"

clang++ -std=c++17 -arch arm64 -mmacosx-version-min=12.0 -fvisibility=default -O2 \
  -I "$SDK/photoshopapi/photoshop" \
  -I "$SDK/photoshopapi/pica_sp" \
  -I "$SDK/photoshopapi/resources" \
  -I "$SDK/samplecode/common/includes" \
  -bundle -framework CoreFoundation \
  "$HERE/src/RustExposure.cpp" \
  "$ROOT/target/release/libengine_ffi.a" \
  -o "$OUT/Contents/MacOS/RustExposure"

echo "built: $OUT"
