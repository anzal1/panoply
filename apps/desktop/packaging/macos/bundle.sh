#!/bin/bash
# Builds Panoply.app: the release binary, the community example modules, an Info.plist with
# LSUIElement false (Dock icon and app window; the command bar works from the hotkey), and an ad-hoc
# signature. Nothing is launched.
#
#   packaging/macos/bundle.sh                       build with the workspace release profile, then bundle
#   packaging/macos/bundle.sh --binary PATH         bundle an existing binary (no build)
#   packaging/macos/bundle.sh --sign "Developer ID Application: Name (TEAM)"   sign for distribution
#
# Output: apps/desktop/dist/Panoply.app. Notarization is a separate step (see packaging/README.md).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../../.." && pwd)"
APPDIR="$ROOT/apps/desktop"
BINARY=""; SIGN="-"
while [ $# -gt 0 ]; do
  case "$1" in
    --binary) BINARY="$2"; shift 2 ;;
    --sign) SIGN="$2"; shift 2 ;;
    *) echo "unknown option $1"; exit 2 ;;
  esac
done
if [ -z "$BINARY" ]; then
  (cd "$ROOT" && nice -n 10 cargo build --release -j4 -p panoply-desktop)
  BINARY="$ROOT/target/release/panoply-desktop"
fi
[ -x "$BINARY" ] || { echo "no binary at $BINARY"; exit 1; }
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
APP="$APPDIR/dist/Panoply.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/modules"
cp "$BINARY" "$APP/Contents/MacOS/Panoply"
sed "s/@VERSION@/$VERSION/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null
# License files and third-party notices travel inside the app (Resources/ is visible in Finder with
# Show Package Contents). THIRD-PARTY-NOTICES.md is generated: tools/notices/generate.py.
for f in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md; do
  [ -f "$ROOT/$f" ] || { echo "missing $ROOT/$f"; exit 1; }
  cp "$ROOT/$f" "$APP/Contents/Resources/$f"
done
# Community example modules: one folder each, manifest plus the compiled component next to it.
for m in "$ROOT"/modules/examples-wasm/*/; do
  name="$(basename "$m")"
  wasm="$m/target/wasm32-wasip2/release/module.wasm"
  if [ -f "$m/module.toml" ] && [ -f "$wasm" ]; then
    mkdir -p "$APP/Contents/Resources/modules/$name"
    cp "$m/module.toml" "$APP/Contents/Resources/modules/$name/"
    cp "$wasm" "$APP/Contents/Resources/modules/$name/module.wasm"
  fi
done
codesign --force --options runtime --sign "$SIGN" "$APP" >/dev/null
echo "Built $APP ($(du -sh "$APP" | cut -f1))"
