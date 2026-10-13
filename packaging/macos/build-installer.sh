#!/bin/bash
# Builds the Citrine Labs installer (.pkg): the app in /Applications, plus the Photoshop plug-in
# that the post-install script copies into every installed Photoshop Plug-ins folder.
#
# Usage: packaging/macos/build-installer.sh [path/to/Citrine.plugin]
# The app bundle must already be built (npx tauri build --bundles app).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="$ROOT/target/release/bundle/macos/Citrine Labs.app"
PLUGIN="${1:-$ROOT/../Citrine Build/build/Release/Citrine.plugin}"
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")"
OUT="$ROOT/target/release/bundle/pkg"
STAGE="$ROOT/target/pkg-stage"

[ -d "$APP" ] || { echo "missing app bundle: $APP"; exit 1; }
[ -d "$PLUGIN" ] || { echo "missing plug-in bundle: $PLUGIN"; exit 1; }

rm -rf "$STAGE" && mkdir -p "$STAGE/app/Applications" "$STAGE/plugin/Library/Application Support/Citrine Labs/Photoshop" "$OUT"
ditto "$APP" "$STAGE/app/Applications/Citrine Labs.app"
# Shipped as a plain folder, not a .plugin bundle, so the Installer does not treat it as a bundle to relocate.
ditto "$PLUGIN" "$STAGE/plugin/Library/Application Support/Citrine Labs/Photoshop/Citrine-plugin"

# Bundles are not relocatable: the Installer must write each one to its fixed path, never to
# another copy it finds elsewhere on the disk.
component_plist() { # $1 = output file, $2 = bundle path relative to the root
  cat > "$1" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><array><dict>
  <key>BundleIsRelocatable</key><false/>
  <key>BundleIsVersionChecked</key><false/>
  <key>BundleHasStrictIdentifier</key><false/>
  <key>BundleOverwriteAction</key><string>upgrade</string>
  <key>RootRelativeBundlePath</key><string>$2</string>
</dict></array></plist>
PLIST
}
component_plist "$STAGE/app.plist" "Applications/Citrine Labs.app"

# Component 1: the app. Component 2: the plug-in payload, which the post-install script installs.
pkgbuild --root "$STAGE/app" --component-plist "$STAGE/app.plist" --identifier com.citrinelabs.app \
  --version "$VERSION" --install-location / "$STAGE/citrine-app.pkg"
pkgbuild --root "$STAGE/plugin" --identifier com.citrinelabs.photoshop-plugin \
  --version "$VERSION" --install-location / --scripts "$ROOT/packaging/macos/scripts" "$STAGE/citrine-plugin.pkg"

cat > "$STAGE/distribution.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
  <title>Citrine Labs</title>
  <options customize="never" require-scripts="false" hostArchitectures="arm64"/>
  <domains enable_localSystem="true"/>
  <choices-outline><line choice="app"/><line choice="plugin"/></choices-outline>
  <choice id="app" title="Citrine Labs"><pkg-ref id="com.citrinelabs.app"/></choice>
  <choice id="plugin" title="Photoshop plug-in"><pkg-ref id="com.citrinelabs.photoshop-plugin"/></choice>
  <pkg-ref id="com.citrinelabs.app" version="$VERSION" onConclusion="none">citrine-app.pkg</pkg-ref>
  <pkg-ref id="com.citrinelabs.photoshop-plugin" version="$VERSION" onConclusion="none">citrine-plugin.pkg</pkg-ref>
</installer-gui-script>
XML

productbuild --distribution "$STAGE/distribution.xml" --package-path "$STAGE" "$OUT/Citrine-Labs-$VERSION.pkg"
echo "built $OUT/Citrine-Labs-$VERSION.pkg"
