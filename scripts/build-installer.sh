#!/bin/bash
# Builds Citrine Labs.app with the Photoshop plug-in inside it, then a .pkg that installs the app
# and copies the plug-in into every installed Photoshop.
# Usage: scripts/build-installer.sh <path to built Citrine.plugin>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLUGIN="${1:?usage: scripts/build-installer.sh <path to Citrine.plugin>}"
[ -d "$PLUGIN" ] || { echo "no plug-in at $PLUGIN"; exit 1; }
export PATH="$HOME/.cargo/bin:$PATH"

(cd "$ROOT/apps/desktop" && npm run build && npx tauri build --bundles app)

OUT="$ROOT/target/installer"
STAGE="$OUT/root"
rm -rf "$OUT"
mkdir -p "$STAGE/Applications"
cp -R "$ROOT/target/release/bundle/macos/Citrine Labs.app" "$STAGE/Applications/"
cp -R "$PLUGIN" "$STAGE/Applications/Citrine Labs.app/Contents/Resources/"
# Adding the plug-in changes the app's contents, so the ad-hoc signature is redone.
codesign --force --deep --sign - "$STAGE/Applications/Citrine Labs.app"

pkgbuild --root "$STAGE" \
    --scripts "$ROOT/scripts/installer" \
    --identifier local.citrine.labs.installer \
    --version 0.1.0 \
    --install-location / \
    "$OUT/CitrineLabs-0.1.0.pkg"
echo "Built $OUT/CitrineLabs-0.1.0.pkg"
