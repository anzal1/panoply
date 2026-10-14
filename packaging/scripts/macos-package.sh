#!/bin/bash
# Packages the macOS artifacts for one architecture from binaries already built into --stage.
#
#   macos-package.sh --version 0.1.0-beta.1 --arch arm64|x64 --stage DIR --out DIR --desktop real|stub|none
#
# --stage holds `panoply` (the CLI) and, with --desktop real, `panoply-desktop`, both built for --arch.
#   real  builds Panoply.app with apps/desktop/packaging/macos/bundle.sh
#   stub  builds a throwaway app around the CLI to exercise the zip, dmg and signing paths while the
#         desktop crate is absent. Outputs get a `selftest-` prefix and are never released.
#   none  CLI archive only
# Output in --out: panoply-cli-V-macos-ARCH.tar.gz always, Panoply-V-macos-ARCH.{zip,dmg} with the app.
#
# Releases are unsigned on purpose (no Apple Developer account): the build is ad-hoc signed, which Apple
# Silicon requires for any code to run, and Gatekeeper warns on first open. Real signing and
# notarization run only if these environment variables (repository secrets) are set, and are skipped
# cleanly otherwise. Nothing sets them today:
#   APPLE_CERTIFICATE_P12_BASE64   Developer ID Application certificate and key, exported as .p12, base64
#   APPLE_CERTIFICATE_PASSWORD     the .p12 password
#   APPLE_SIGNING_IDENTITY         e.g. "Developer ID Application: Name (TEAMID)"
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_SPECIFIC_PASSWORD     for notarytool (notarization needs all three)
# See packaging/signing/APPLE.md.
set -euo pipefail

VERSION=""; ARCH=""; STAGE=""; OUT=""; DESKTOP="none"
while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --arch) ARCH="$2"; shift 2 ;;
    --stage) STAGE="$(cd "$2" && pwd)"; shift 2 ;;
    --out) mkdir -p "$2"; OUT="$(cd "$2" && pwd)"; shift 2 ;;
    --desktop) DESKTOP="$2"; shift 2 ;;
    *) echo "unknown option $1"; exit 2 ;;
  esac
done
[ -n "$VERSION" ] && [ -n "$ARCH" ] && [ -n "$STAGE" ] && [ -n "$OUT" ] || { echo "usage: see header"; exit 2; }
case "$ARCH" in
  arm64) LIPO_ARCH=arm64 ;;
  x64) LIPO_ARCH=x86_64 ;;
  *) echo "unknown arch $ARCH"; exit 2 ;;
esac
# The binaries must really be for --arch (the x64 ones are cross-compiled on an arm64 runner).
for b in "$STAGE"/panoply "$STAGE"/panoply-desktop; do
  [ -f "$b" ] || continue
  [ "$(lipo -archs "$b")" = "$LIPO_ARCH" ] || { echo "$b is $(lipo -archs "$b"), expected $LIPO_ARCH"; exit 1; }
done

HERE="$(cd "$(dirname "$0")" && pwd)"
PKG="$(cd "$HERE/.." && pwd)"
ROOT="$(cd "$PKG/.." && pwd)"
WORK="$(mktemp -d)"
STATE="$OUT/state"; mkdir -p "$STATE"
PREFIX=""; [ "$DESKTOP" = stub ] && PREFIX="selftest-"
PY="python3 -I"

# ---- signing set-up -------------------------------------------------------------------------------
SIGNING=0; NOTARIZE=0
if [ -n "${APPLE_CERTIFICATE_P12_BASE64:-}" ] && [ -n "${APPLE_CERTIFICATE_PASSWORD:-}" ] && [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  SIGNING=1
  KC="${RUNNER_TEMP:-$WORK}/panoply-signing.keychain-db"
  KCPW="$(uuidgen)"
  security create-keychain -p "$KCPW" "$KC"
  security set-keychain-settings -lut 21600 "$KC"
  security unlock-keychain -p "$KCPW" "$KC"
  printf '%s' "$APPLE_CERTIFICATE_P12_BASE64" | base64 --decode > "$WORK/cert.p12"
  security import "$WORK/cert.p12" -k "$KC" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
  rm -f "$WORK/cert.p12"
  security set-key-partition-list -S apple-tool:,apple: -s -k "$KCPW" "$KC" >/dev/null
  # shellcheck disable=SC2046
  security list-keychains -d user -s "$KC" $(security list-keychains -d user | tr -d '"')
  if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_SPECIFIC_PASSWORD:-}" ] && [ "$DESKTOP" != stub ]; then
    NOTARIZE=1
  fi
fi
if [ "$SIGNING" = 0 ]; then
  echo "::notice title=macOS signing skipped::Unsigned on purpose: no Developer ID secrets, so the build is ad-hoc signed and not notarized. Users get the first-open prompt described in the release notes."
elif [ "$NOTARIZE" = 0 ] && [ "$DESKTOP" != stub ]; then
  echo "::notice title=macOS notarization skipped::Signed with Developer ID, but APPLE_ID, APPLE_TEAM_ID or APPLE_APP_SPECIFIC_PASSWORD is missing."
fi
echo "NOTARIZED=$NOTARIZE" > "$STATE/macos-$ARCH.env"

sign() {
  if [ "$SIGNING" = 1 ]; then
    codesign --force --timestamp --options runtime --entitlements "$PKG/macos/entitlements.plist" \
      --sign "$APPLE_SIGNING_IDENTITY" "$@"
  else
    codesign --force --sign - "$@"
  fi
}

# Submit a file to Apple's notary service and fail the build unless it is accepted.
notarize() {
  local file="$1" json status id
  json="$(xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" --wait --timeout 40m --output-format json)"
  status="$(printf '%s' "$json" | $PY -c 'import json,sys; print(json.load(sys.stdin).get("status",""))')"
  id="$(printf '%s' "$json" | $PY -c 'import json,sys; print(json.load(sys.stdin).get("id",""))')"
  echo "notarization of $(basename "$file"): $status ($id)"
  if [ "$status" != "Accepted" ]; then
    xcrun notarytool log "$id" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
      --password "$APPLE_APP_SPECIFIC_PASSWORD" || true
    exit 1
  fi
}

# ---- CLI ------------------------------------------------------------------------------------------
mkdir -p "$WORK/cli"
cp "$STAGE/panoply" "$WORK/cli/panoply"
chmod +x "$WORK/cli/panoply"
sign "$WORK/cli/panoply"
lipo -archs "$WORK/cli/panoply"
if [ "$NOTARIZE" = 1 ]; then
  # A bare binary cannot be stapled; notarizing it still lets Gatekeeper find the ticket online.
  ditto -c -k "$WORK/cli/panoply" "$WORK/panoply-cli.zip"
  notarize "$WORK/panoply-cli.zip"
fi
CLI_NAME="panoply-cli-$VERSION-macos-$ARCH"
EXTRA=()
[ -f "$ROOT/THIRD-PARTY-NOTICES.md" ] && EXTRA+=("$ROOT/THIRD-PARTY-NOTICES.md")
$PY "$HERE/mkarchive.py" "$OUT/$CLI_NAME.tar.gz" "$CLI_NAME" \
  "$WORK/cli/panoply=panoply" "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/README.md" ${EXTRA[@]+"${EXTRA[@]}"}

[ "$DESKTOP" = none ] && exit 0

# ---- Panoply.app ----------------------------------------------------------------------------------
APPW="$WORK/Panoply.app"
if [ "$DESKTOP" = real ]; then
  chmod +x "$STAGE/panoply-desktop"   # artifacts lose the executable bit in transit
  "$ROOT/apps/desktop/packaging/macos/bundle.sh" --binary "$STAGE/panoply-desktop" --sign -
  ditto "$ROOT/apps/desktop/dist/Panoply.app" "$APPW"
else
  mkdir -p "$APPW/Contents/MacOS" "$APPW/Contents/Resources"
  cp "$WORK/cli/panoply" "$APPW/Contents/MacOS/Panoply"
  cat > "$APPW/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Panoply</string>
  <key>CFBundleIdentifier</key><string>dev.panoply.selftest</string>
  <key>CFBundleExecutable</key><string>Panoply</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
</dict></plist>
PLIST
fi
for f in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md; do
  [ -f "$ROOT/$f" ] && [ ! -f "$APPW/Contents/Resources/$f" ] && cp "$ROOT/$f" "$APPW/Contents/Resources/$f"
done
# The command line tool rides inside the app so `brew install --cask panoply` can link it. Its name
# differs from the app executable (`Panoply`) because macOS volumes are usually case-insensitive.
cp "$WORK/cli/panoply" "$APPW/Contents/MacOS/panoply-cli"

# An app icon, unless the app already ships one.
if ! ls "$APPW"/Contents/Resources/*.icns >/dev/null 2>&1; then
  ICON_SRC="$ROOT/apps/desktop/packaging/icons/panoply-512.png"
  [ -f "$ICON_SRC" ] || ICON_SRC="$PKG/assets/panoply-512.png"
  SET="$WORK/Panoply.iconset"; mkdir -p "$SET"
  for s in 16 32 64 128 256 512; do sips -z $s $s "$ICON_SRC" --out "$SET/icon_${s}x${s}.png" >/dev/null; done
  cp "$SET/icon_32x32.png" "$SET/icon_16x16@2x.png"
  cp "$SET/icon_64x64.png" "$SET/icon_32x32@2x.png"
  cp "$SET/icon_256x256.png" "$SET/icon_128x128@2x.png"
  cp "$SET/icon_512x512.png" "$SET/icon_256x256@2x.png"
  sips -z 1024 1024 "$ICON_SRC" --out "$SET/icon_512x512@2x.png" >/dev/null
  rm "$SET/icon_64x64.png"
  iconutil -c icns "$SET" -o "$APPW/Contents/Resources/Panoply.icns"
  /usr/libexec/PlistBuddy -c "Delete :CFBundleIconFile" "$APPW/Contents/Info.plist" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string Panoply" "$APPW/Contents/Info.plist"
fi

# Sign inside out: the nested tool first, then the bundle (which signs the main executable).
sign "$APPW/Contents/MacOS/panoply-cli"
sign "$APPW"
codesign --verify --deep --strict --verbose=2 "$APPW"
lipo -archs "$APPW/Contents/MacOS/Panoply"

APP_NAME="${PREFIX}Panoply-$VERSION-macos-$ARCH"
ZIP="$OUT/$APP_NAME.zip"
ditto -c -k --sequesterRsrc --keepParent "$APPW" "$ZIP"
if [ "$NOTARIZE" = 1 ]; then
  notarize "$ZIP"
  xcrun stapler staple "$APPW"
  rm -f "$ZIP"
  ditto -c -k --sequesterRsrc --keepParent "$APPW" "$ZIP"
fi

# ---- .dmg -----------------------------------------------------------------------------------------
DMGDIR="$WORK/dmg"; mkdir -p "$DMGDIR"
ditto "$APPW" "$DMGDIR/Panoply.app"
ln -s /Applications "$DMGDIR/Applications"
DMG="$OUT/$APP_NAME.dmg"
rm -f "$DMG"
hdiutil create -volname "Panoply" -srcfolder "$DMGDIR" -ov -format UDZO "$DMG"
if [ "$SIGNING" = 1 ]; then
  codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$DMG"
fi
if [ "$NOTARIZE" = 1 ]; then
  notarize "$DMG"
  xcrun stapler staple "$DMG"
fi
hdiutil verify "$DMG"
# The app on the disk image carries the notices and the icon.
MNT="$WORK/mnt"; mkdir -p "$MNT"
hdiutil attach "$DMG" -readonly -nobrowse -mountpoint "$MNT" >/dev/null
trap 'hdiutil detach "$MNT" -force >/dev/null 2>&1 || true' EXIT
test -f "$MNT/Panoply.app/Contents/Resources/THIRD-PARTY-NOTICES.md"
ls "$MNT"/Panoply.app/Contents/Resources/*.icns >/dev/null
test -L "$MNT/Applications"
hdiutil detach "$MNT" >/dev/null
trap - EXIT

# ---- smoke test (stub builds only: the real app opens windows) -------------------------------------
if [ "$DESKTOP" = stub ]; then
  "$APPW/Contents/MacOS/panoply-cli" --version
  "$APPW/Contents/MacOS/Panoply" --version
fi
ls -lh "$OUT"
