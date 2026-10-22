#!/bin/bash
# Builds and runs the Citrine settings blob test. Needs no Adobe SDK and no Rust build.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PLUGIN="$(cd "$HERE/.." && pwd)"
ROOT="$(cd "$PLUGIN/../.." && pwd)"
BIN="$ROOT/target/citrine_settings_test"

clang++ -std=c++17 -Wall -Wextra -Werror -O1 \
  -I "$PLUGIN/src" \
  "$HERE/citrine_settings_test.cpp" "$PLUGIN/src/CitrineSettings.cpp" \
  -o "$BIN"
"$BIN"
