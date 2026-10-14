#!/bin/sh
# Rebuilds testdata/labelled: renders the generated text images (clean and degraded) and writes
# their label files. Needs macOS (CoreText) and python3 with Pillow and numpy.
# Usage: sh make_labelled.sh
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/../labelled"
mkdir -p "$out"
tmp="$(mktemp -d)"
swiftc -O "$here/render_text.swift" -o "$tmp/render_text"
python3 "$here/make_spec.py" "$tmp/spec.tsv" "$out"
"$tmp/render_text" "$out" "$tmp/spec.tsv"
python3 "$here/degrade.py" "$out"
rm -rf "$tmp"
