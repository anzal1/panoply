#!/bin/bash
# Automated text-input (IME) and accessibility checks for the kit variant. Never shows the bar.
#   ./selftest.sh            stock gpui-base 0.7.3 from crates.io
#   ./selftest.sh patched    gpui-base with patches/gpui-base-grapheme-and-unmark.diff applied (patches/patched/)
# Builds at nice 19, -j2. Launches a bundled accessory app whose window lives at x = -20000 and is never ordered front,
# calls /imetest /keystest /shapetest /a11ydump on 127.0.0.1:47814, writes JSON to ../../gpui-checks/automated/, quits the app.
set -euo pipefail
cd "$(dirname "$0")"
VARIANT="${1:-stock}"
OUT="$(cd ../.. && pwd)/gpui-checks/automated"
mkdir -p "$OUT"
if pgrep -f 'Arsenal(Kit)?(Selftest)?\.app/Contents/MacOS' >/dev/null; then echo "an Arsenal app is already running, quit it first"; exit 1; fi
LOCK_BAK="$(mktemp)"; cp Cargo.lock "$LOCK_BAK"
if [ "$VARIANT" = patched ]; then
  nice -n 19 cargo build --profile selftest --features selftest-input -j2 --config 'patch.crates-io.gpui-base.path="patches/patched"'
else
  nice -n 19 cargo build --profile selftest --features selftest-input -j2
fi
cp "$LOCK_BAK" Cargo.lock; rm -f "$LOCK_BAK"
APP=ArsenalKitSelftest.app
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/selftest/arsenal-gpui-kit "$APP/Contents/MacOS/ArsenalKitSelftest"
cp ../../shared/utilities.json "$APP/Contents/Resources/utilities.json"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>ArsenalKitSelftest</string>
  <key>CFBundleIdentifier</key><string>dev.shelltest.arsenal.gpui.kit.selftest</string>
  <key>CFBundleExecutable</key><string>ArsenalKitSelftest</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSUIElement</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1
(ARSENAL_APPEARANCE=light nice -n 19 "./$APP/Contents/MacOS/ArsenalKitSelftest" >"$OUT/$VARIANT-app.log" 2>&1 &)
trap 'pkill -f ArsenalKitSelftest.app/Contents/MacOS 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do sleep 0.5; curl -s -m 3 127.0.0.1:47814/state >"$OUT/$VARIANT-state-before.json" && break; done
cat "$OUT/$VARIANT-state-before.json"; echo
for ep in imetest keystest shapetest a11ydump; do
  curl -s -m 170 "127.0.0.1:47814/$ep" >"$OUT/$VARIANT-$ep.json"
  echo "$ep -> $OUT/$VARIANT-$ep.json"
done
curl -s -m 3 127.0.0.1:47814/state >"$OUT/$VARIANT-state-after.json"; cat "$OUT/$VARIANT-state-after.json"; echo
