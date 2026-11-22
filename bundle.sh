#!/bin/bash
# Builds SnapRec.app and SnapRec.dmg in ./dist
#   ./bundle.sh             build only
#   ./bundle.sh --install   build, then install to /Applications and launch
set -euo pipefail
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

INSTALL=0
[ "${1:-}" = "--install" ] && INSTALL=1

command -v cargo >/dev/null || { echo "Rust is not installed: see https://rustup.rs" >&2; exit 1; }
command -v swiftc >/dev/null || { echo "Xcode Command Line Tools missing: xcode-select --install" >&2; exit 1; }

APP=dist/SnapRec.app
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

cargo build --release
swiftc -O -target arm64-apple-macos13.0 helper/main.swift -o target/release/snaprec-helper

rm -rf dist && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/snaprec "$APP/Contents/MacOS/snaprec"
cp target/release/snaprec-helper "$APP/Contents/MacOS/snaprec-helper"

# App icon (panda) from assets/icon_1024.png
ICONSET=$(mktemp -d)/AppIcon.iconset && mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icon_1024.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) assets/icon_1024.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>SnapRec</string>
    <key>CFBundleDisplayName</key><string>SnapRec</string>
    <key>CFBundleIdentifier</key><string>com.snaprec.app</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleExecutable</key><string>snaprec</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSScreenCaptureUsageDescription</key><string>SnapRec needs screen access to take screenshots and record video.</string>
</dict>
</plist>
EOF

# Extended attributes (quarantine, Finder info) make codesign fail and make
# Gatekeeper refuse to open the app ("Apple could not verify…").
xattr -cr "$APP"

# Sign with a stable local identity so macOS keeps the Screen Recording
# permission across rebuilds (ad-hoc signatures change every build).
sign_app() { # $1 = identity ("-" for ad-hoc)
  codesign --force --sign "$1" --identifier com.snaprec.app.helper "$APP/Contents/MacOS/snaprec-helper" &&
  codesign --force --sign "$1" --identifier com.snaprec.app "$APP"
}
SIGN_ID="${SIGN_ID:-SnapRec Local Signing}"
if ./tools/ensure_signing_identity.sh "$SIGN_ID" && sign_app "$SIGN_ID"; then
  echo "Signed with \"$SIGN_ID\"."
else
  echo "WARNING: could not sign with \"$SIGN_ID\"; falling back to ad-hoc signing." >&2
  echo "         Screen Recording permission will need re-approval after each rebuild." >&2
  sign_app -
fi
codesign --verify --deep --strict "$APP"

# Disk image with drag-to-Applications
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname SnapRec -srcfolder "$STAGE" -ov -format UDZO dist/SnapRec.dmg >/dev/null 2>&1
rm -rf "$STAGE"

echo "Built: $(pwd)/$APP"
echo "Built: $(pwd)/dist/SnapRec.dmg"

if [ "$INSTALL" = 1 ]; then
  pkill -x snaprec 2>/dev/null && sleep 1 || true
  rm -rf /Applications/SnapRec.app
  ditto "$APP" /Applications/SnapRec.app
  xattr -dr com.apple.quarantine /Applications/SnapRec.app 2>/dev/null || true
  echo "Installed: /Applications/SnapRec.app"
  open /Applications/SnapRec.app
else
  echo "Install: ./bundle.sh --install   (or: ditto $APP /Applications/SnapRec.app)"
fi
