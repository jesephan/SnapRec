#!/bin/bash
# Builds SnapRec.app and SnapRec.dmg in ./dist
set -euo pipefail
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

APP=dist/SnapRec.app
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

cargo build --release

rm -rf dist && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/snaprec "$APP/Contents/MacOS/snaprec"

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
    <key>LSMinimumSystemVersion</key><string>11.0</string>
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
hdiutil create -volname SnapRec -srcfolder "$STAGE" -ov -format UDZO dist/SnapRec.dmg >/dev/null
rm -rf "$STAGE"

echo "Built: $(pwd)/$APP"
echo "Built: $(pwd)/dist/SnapRec.dmg"
echo "Install: cp -R $APP /Applications/"
