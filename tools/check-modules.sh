#!/usr/bin/env bash
# The checks for the community modules in modules/examples-wasm, one Cargo project each:
# fmt, clippy (-D warnings), the module's own unit tests on the host, and the WebAssembly build.
# CI runs it (the `modules` job) and tools/preflight.sh runs it when a module changed.
#
#     tools/check-modules.sh              every module
#     tools/check-modules.sh qr-code      only the named ones
set -euo pipefail
cd "$(git rev-parse --show-toplevel)/modules/examples-wasm"

if [ "$#" -gt 0 ]; then
  mods=("$@")
else
  mods=()
  for d in */; do mods+=("${d%/}"); done
fi

for m in "${mods[@]}"; do
  echo "==> $m"
  (
    cd "$m"
    cargo fmt --check
    cargo clippy --quiet --all-targets -- -D warnings
    cargo test --quiet
    cargo build --release --target wasm32-wasip2 --quiet
    ls -l target/wasm32-wasip2/release/module.wasm | awk '{print "    module.wasm " $5 " bytes"}'
  )
done
echo "modules: all green"
