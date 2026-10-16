#!/usr/bin/env bash
# Runs inside the container of tools/preflight-linux.sh. Not meant to be run by hand.
set -euo pipefail

export CARGO_TARGET_DIR=/cache/target CARGO_BUILD_JOBS="${JOBS:-8}" PANOPLY_YES=1
# lld, not GNU ld: parallel links of the big test binaries were OOM-killed in the 4 GB VM.
# Set for the Linux target only: the wasm32-wasip2 modules that tests build link with wasm-component-ld,
# which rejects -fuse-ld, so a plain RUSTFLAGS broke them.
case "$(uname -m)" in
  aarch64 | arm64) export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=lld" ;;
  *) export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=lld" ;;
esac
export PANOPLY_ENGINES_DIR=/cache/engines PANOPLY_DATA_DIR=/tmp/panoply-data
start=$SECONDS
tests_failed=0
step() { printf '\n--> %s\n' "$*"; step_start=$SECONDS; }
done_step() { printf '    %ss\n' "$((SECONDS - step_start))"; }

# The VM's memory, sampled every 2 s; the peak is printed when this script ends.
peak_file=/tmp/mem-peak
echo 0 >"$peak_file"
(
  while :; do
    used=$(awk '/^MemTotal/{t=$2} /^MemAvailable/{a=$2} END{print int((t-a)/1024)}' /proc/meminfo)
    [ "$used" -gt "$(cat "$peak_file")" ] && echo "$used" >"$peak_file"
    sleep 2
  done
) &
sampler=$!
finish() {
  status=$?
  kill "$sampler" 2>/dev/null || true
  total=$(awk '/^MemTotal/{print int($2/1024)}' /proc/meminfo)
  printf '\n    peak VM memory in use: %s MiB of %s MiB; inside the container %ss\n' "$(cat "$peak_file")" "$total" "$((SECONDS - start))"
  exit "$status"
}
trap finish EXIT

step "syncing the repo into the work volume"
mkdir -p /work
rsync -a --delete --exclude .git --exclude '.DS_Store' --exclude 'target/' --exclude /tests/corpus/cache /src/ /work/
cd /work
# The corpus kinds CI has (pdf, image), mounted read-only from the Mac's cache.
rm -rf tests/corpus/cache && mkdir -p tests/corpus/cache
for kind in pdf image; do
  [ -d "/corpus/$kind" ] && ln -s "/corpus/$kind" "tests/corpus/cache/$kind"
done
done_step

if [ -n "${PLAN:-}" ]; then
  # The WebAssembly modules some tests build, built once here as CI does (they live in the work volume).
  if [ "$SCOPE" = "--workspace --exclude gpui-base" ] || [[ " $SCOPE " == *" panoply-wasm "* ]]; then
    step "building the WebAssembly test modules"
    for d in crates/panoply-wasm/tests/fixtures/* modules/examples-wasm/*; do
      [ -f "$d/Cargo.toml" ] && nice -n 15 cargo build --release --target wasm32-wasip2 --quiet --manifest-path "$d/Cargo.toml"
    done
    done_step
  fi
fi

step "cargo clippy --workspace --exclude gpui-base --all-targets -- -D warnings"
nice -n 15 cargo clippy --workspace --exclude gpui-base --all-targets -- -D warnings
done_step

if [ -n "${PLAN:-}" ]; then
  step "cargo nextest run $SCOPE (profile $NEXTEST_PROFILE)"
  # shellcheck disable=SC2086
  nice -n 15 cargo nextest run $SCOPE --cargo-profile ci --profile "$NEXTEST_PROFILE" --lib --bins --tests --no-fail-fast --no-tests=pass || tests_failed=1
  done_step

  step "timing tests, alone (profile ci-idle)"
  # shellcheck disable=SC2086
  nice -n 15 cargo nextest run $SCOPE --cargo-profile ci --profile ci-idle --lib --bins --tests --no-fail-fast --no-tests=pass || tests_failed=1
  done_step
fi

if [ "${SMOKE:-0}" = 1 ]; then
  step "building panoply-desktop"
  nice -n 15 cargo build --profile ci -p panoply-desktop
  done_step
  step "headless desktop smoke (Xvfb + openbox)"
  bash /work/tools/linux/smoke.sh "$CARGO_TARGET_DIR/ci/panoply-desktop" /out
  done_step
fi

if [ "$tests_failed" = 1 ]; then
  echo "tests FAILED (see above)"
  exit 1
fi
