#!/bin/bash
# Builds the release binary at low priority and wraps it into Arsenal.app (LSUIElement accessory app).
set -euo pipefail
cd "$(dirname "$0")"
nice -n 10 swift build -c release --product Arsenal
APP="Arsenal.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp .build/release/Arsenal "$APP/Contents/MacOS/Arsenal"
cp ../shared/utilities.json "$APP/Contents/Resources/utilities.json"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Arsenal</string>
  <key>CFBundleDisplayName</key><string>Arsenal</string>
  <key>CFBundleIdentifier</key><string>dev.shelltest.arsenal.swiftui</string>
  <key>CFBundleExecutable</key><string>Arsenal</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>26.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1
echo "Built $(pwd)/$APP"
