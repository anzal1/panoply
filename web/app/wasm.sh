#!/bin/sh
# Rebuild the wasm engines and refresh app/src/*wasm. Run from web/. CARGO_BUILD_JOBS keeps a laptop cool.
set -e
cd "$(dirname "$0")/.."
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
cargo build --release --target wasm32-unknown-unknown
T="${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release"
for c in pdf:pdf vision:vis ocr:ocr id:id media:media; do
  crate="${c%%:*}"; out="${c##*:}"
  mkdir -p "app/src/${out}wasm"
  wasm-bindgen --target web --no-typescript --out-dir "app/src/${out}wasm" "$T/panoply_web_${crate}.wasm"
  f="app/src/${out}wasm/panoply_web_${crate}_bg.wasm"
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-simd -o "$f" "$f"
done
