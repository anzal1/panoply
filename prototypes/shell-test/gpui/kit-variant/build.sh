#!/bin/bash
# Builds the release binary at low priority (max 2 jobs) and wraps it into ArsenalKit.app (LSUIElement accessory app).
set -euo pipefail
cd "$(dirname "$0")"
nice -n 19 cargo build --release -j2
APP="ArsenalKit.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/arsenal-gpui-kit "$APP/Contents/MacOS/ArsenalKit"
cp ../../shared/utilities.json "$APP/Contents/Resources/utilities.json"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>ArsenalKit</string>
  <key>CFBundleDisplayName</key><string>ArsenalKit</string>
  <key>CFBundleIdentifier</key><string>dev.shelltest.arsenal.gpui.kit</string>
  <key>CFBundleExecutable</key><string>ArsenalKit</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1
echo "Built $(pwd)/$APP"
