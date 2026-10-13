#!/bin/bash
# Offscreen 3x renders of the bar (light and dark) into ./snapshots, using the real engine and fonts.
cd "$(dirname "$0")"
APP=build/macos/Build/Products/Release/arsenal_bar.app
rm -rf snapshots && mkdir -p snapshots
ARSENAL_SNAPSHOT_DIR="$PWD/snapshots" "$APP/Contents/MacOS/arsenal_bar" >/dev/null 2>&1
ls snapshots
