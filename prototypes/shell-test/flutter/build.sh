#!/bin/bash
# Release build of the Flutter command bar WITHOUT a full Xcode install.
#
# `flutter build macos` needs xcodebuild, which only ships with Xcode. This
# machine only has the Command Line Tools, so this script runs the same steps
# the Xcode project would:
#   1. flutter assemble   -> App.framework (AOT, arm64, release) + flutter_assets
#   2. swiftc             -> the native Runner (window, glass material, hotkey, drops)
#   3. bundle + ad-hoc codesign
# With Xcode installed, `flutter build macos --release` builds the same sources.
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$PWD"
APP_NAME=arsenal_bar
OUT="$ROOT/build/manual/out"
APP="$ROOT/build/macos/Build/Products/Release/$APP_NAME.app"
SDK="$(xcrun --sdk macosx --show-sdk-path)"

# the data file is the shared one, bundled as an asset
cp ../shared/utilities.json assets/utilities.json

rm -rf "$OUT" && mkdir -p "$OUT"
nice -n 10 flutter assemble --no-version-check --output="$OUT/" \
  -dTargetPlatform=darwin -dTargetFile=lib/main.dart -dBuildMode=release \
  -dConfiguration=Release -dDarwinArchs=arm64 -dTreeShakeIcons=true \
  -dSrcRoot="$ROOT/macos" release_macos_bundle_flutter_assets

rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"

nice -n 10 swiftc -O -wmo -parse-as-library \
  -sdk "$SDK" -target arm64-apple-macos13.0 \
  -F "$OUT" -framework FlutterMacOS \
  -Xlinker -rpath -Xlinker @executable_path/../Frameworks \
  -module-name "$APP_NAME" \
  macos/Runner/AppDelegate.swift macos/Runner/MainFlutterWindow.swift \
  -o "$APP/Contents/MacOS/$APP_NAME"

ditto "$OUT/App.framework" "$APP/Contents/Frameworks/App.framework"
ditto "$OUT/FlutterMacOS.framework" "$APP/Contents/Frameworks/FlutterMacOS.framework"

sed -e 's/\$(DEVELOPMENT_LANGUAGE)/en/' -e "s/\$(EXECUTABLE_NAME)/$APP_NAME/" \
    -e 's/\$(PRODUCT_BUNDLE_IDENTIFIER)/dev.arsenal.arsenalBar/' -e "s/\$(PRODUCT_NAME)/$APP_NAME/" \
    -e 's/\$(FLUTTER_BUILD_NAME)/1.0.0/' -e 's/\$(FLUTTER_BUILD_NUMBER)/1/' \
    -e 's/\$(MACOSX_DEPLOYMENT_TARGET)/13.0/' -e 's/\$(PRODUCT_COPYRIGHT)/Arsenal/' \
    macos/Runner/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

codesign --force --deep --sign - "$APP"
echo "built: $APP"
du -sh "$APP"
