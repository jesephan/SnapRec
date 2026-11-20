#!/bin/bash
# Builds SnapRec.app and SnapRec.dmg in ./dist
set -euo pipefail
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

APP=dist/SnapRec.app
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

cargo build --release
swiftc -O -target arm64-apple-macos13.0 helper/main.swift -o target/release/snaprec-helper

rm -rf dist && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/snaprec "$APP/Contents/MacOS/snaprec"
cp target/release/snaprec-helper "$APP/Contents/MacOS/snaprec-helper"

# App icon (bird) from assets/icon_1024.png
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

# Ad-hoc sign so macOS can remember the Screen Recording permission
codesign --force --deep --sign - "$APP"

# Disk image with drag-to-Applications
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname SnapRec -srcfolder "$STAGE" -ov -format UDZO dist/SnapRec.dmg >/dev/null 2>&1
rm -rf "$STAGE"

echo "Built: $(pwd)/$APP"
echo "Built: $(pwd)/dist/SnapRec.dmg"
echo "Install: cp -R $APP /Applications/"
