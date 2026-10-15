#!/usr/bin/env bash
# Builds the modules listed in modules/shipped.txt for wasm32-wasip2 and lays them out the way every
# release bundle expects them:
#
#     tools/build-shipped-modules.sh OUT_DIR
#     OUT_DIR/<folder>/module.toml
#     OUT_DIR/<folder>/module.wasm
#
# The release workflow runs this once (WebAssembly does not depend on the platform) and every
# packaging script copies OUT_DIR into its bundle. It fails when a listed folder holds another module id,
# when a build is missing, and when OUT_DIR would hold anything but the listed modules.
set -euo pipefail
[ $# -eq 1 ] || { echo "usage: $0 OUT_DIR"; exit 2; }
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$1"
OUT="$(cd "$1" && pwd)"
ls "$OUT" | grep -q . && { echo "$OUT is not empty"; exit 2; }

listed=()
while read -r folder id _; do
  case "$folder" in ''|'#'*) continue ;; esac
  src="$ROOT/modules/examples-wasm/$folder"
  [ -f "$src/module.toml" ] || { echo "modules/shipped.txt lists $folder, which is not in modules/examples-wasm"; exit 1; }
  grep -q "^id = \"$id\"" "$src/module.toml" || { echo "$folder/module.toml does not declare id $id"; exit 1; }
  # --locked: the committed Cargo.lock of the module is what ships (and what THIRD-PARTY-NOTICES.md lists).
  (cd "$src" && env -u CARGO_TARGET_DIR cargo build --release --locked --target wasm32-wasip2 --quiet)
  wasm="$src/target/wasm32-wasip2/release/module.wasm"
  [ -s "$wasm" ] || { echo "$folder did not build: no $wasm"; exit 1; }
  mkdir -p "$OUT/$folder"
  cp "$src/module.toml" "$OUT/$folder/module.toml"
  cp "$wasm" "$OUT/$folder/module.wasm"
  listed+=("$folder")
  echo "built $folder ($id), $(wc -c < "$wasm" | tr -d ' ') bytes"
done < "$ROOT/modules/shipped.txt"

[ "${#listed[@]}" -gt 0 ] || { echo "modules/shipped.txt lists nothing"; exit 1; }
found="$(cd "$OUT" && ls | sort | tr '\n' ' ')"
want="$(printf '%s\n' "${listed[@]}" | sort | tr '\n' ' ')"
[ "$found" = "$want" ] || { echo "unexpected content in $OUT: $found (wanted $want)"; exit 1; }
echo "shipped modules: $found"
