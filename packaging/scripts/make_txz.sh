#!/bin/bash
# Adds a .tar.xz beside the archives a packaging script already wrote into --dir. They are far smaller
# than the .zip and .tar.gz (the macOS app is about 13 MB against 31 MB) and unpack in under a second
# with the tar that every Mac and Linux machine already has. Existing files are never touched.
#
#   make_txz.sh --dir DIR
#
#   X.tar.gz            becomes X.tar.xz: the same tar stream, so contents, modes and layout are identical
#   Panoply-*-macos-*.zip   becomes a .tar.xz of Panoply.app: unpacked with ditto (so symlinks, modes and
#                           the ad-hoc signature survive), then tarred. Built with tar, not zip, so
#                           symlinks and executable bits are kept as they are.
# Windows is not handled on purpose: run this in the macOS and Linux package jobs only.
set -euo pipefail

DIR=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dir) DIR="$(cd "$2" && pwd)"; shift 2 ;;
    *) echo "unknown option $1"; exit 2 ;;
  esac
done
[ -n "$DIR" ] || { echo "usage: make_txz.sh --dir DIR"; exit 2; }

command -v xz >/dev/null || { echo "xz is not installed"; exit 1; }
export COPYFILE_DISABLE=1   # no AppleDouble (._*) entries from bsdtar on macOS
shopt -s nullglob
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
made=0

finish() {  # temporary file, final file
  xz -t "$1"
  mv "$1" "$2"
  made=$((made + 1))
  echo "wrote $2 ($(wc -c < "$2" | tr -d ' ') bytes)"
}

for gz in "$DIR"/*.tar.gz; do
  out="${gz%.tar.gz}.tar.xz"
  gzip -dc "$gz" | xz -9e -T0 -c > "$out.part"
  finish "$out.part" "$out"
done

for zip in "$DIR"/*Panoply-*-macos-*.zip; do
  out="${zip%.zip}.tar.xz"
  unpacked="$WORK/$(basename "$zip" .zip)"
  mkdir -p "$unpacked"
  ditto -x -k "$zip" "$unpacked"
  [ -d "$unpacked/Panoply.app" ] || { echo "$zip does not hold Panoply.app"; exit 1; }
  tar -cf - -C "$unpacked" Panoply.app | xz -9e -T0 -c > "$out.part"
  finish "$out.part" "$out"
  rm -rf "$unpacked"
done

[ "$made" -gt 0 ] || { echo "no .tar.gz or macOS .zip in $DIR"; exit 1; }
