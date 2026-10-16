#!/usr/bin/env bash
# The disk map scan benchmark: time, memory and accuracy against `du -sk` on one tree.
#
#     tools/disk-bench.sh <path> [runs]        scan <path> `runs` times per setting (default 5)
#
# Builds the scan example in release mode, then for the default backend at several thread counts, the portable
# backend, and `du -skx`, prints the median wall time. Peak memory comes from /usr/bin/time (macOS: -l, Linux: -v) on
# one scan. Accuracy is the scan's allocated KiB against du's, with the clone bytes and the hard link bytes shown,
# since `du` counts APFS clones twice and the scan does not. Runs under the machine-wide heavy lock at nice 15.
set -euo pipefail
target="${1:?usage: disk-bench.sh <path> [runs]}"
runs="${2:-5}"
cd "$(git rev-parse --show-toplevel)"
. tools/lib/heavy-lock.sh
heavy_lock_acquire
trap heavy_lock_release EXIT
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/personal/panoply-shared.noindex}" CARGO_BUILD_JOBS=8
nice -n 15 cargo build --release -p panoply-disk --example scan -j 8 2>&1 | tail -1
bin="$CARGO_TARGET_DIR/release/examples/scan"

median() { sort -n | awk '{a[NR]=$1} END {print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}'; }
timeit() { # command... -> seconds, one run
  local s e
  s=$(python3 -c 'import time;print(time.time())')
  "$@" >/dev/null 2>&1 || true
  e=$(python3 -c 'import time;print(time.time())')
  python3 -c "print(round($e-$s,3))"
}

echo "tree: $target"
echo "load before: $(uptime | sed 's/.*load averages*: //')"
nice -n 15 "$bin" "$target" >/dev/null 2>&1 || true # warm the cache
for t in 4 8 16 32 0; do
  label=$([ "$t" = 0 ] && echo "auto" || echo "$t")
  times=$(for _ in $(seq "$runs"); do nice -n 15 "$bin" "$target" --threads "$t" | awk '/scan time/ {print $3}'; done)
  echo "bulk threads=$label  median $(echo "$times" | median) s  (runs: $(echo "$times" | tr '\n' ' '))"
done
times=$(for _ in $(seq "$runs"); do nice -n 15 "$bin" "$target" --no-clones | awk '/scan time/ {print $3}'; done)
echo "bulk threads=auto, clone accounting off  median $(echo "$times" | median) s"
times=$(for _ in $(seq "$runs"); do nice -n 15 "$bin" "$target" --portable | awk '/scan time/ {print $3}'; done)
echo "portable (readdir+lstat) threads=auto  median $(echo "$times" | median) s"
times=$(for _ in $(seq "$runs"); do timeit nice -n 15 du -skx "$target"; done)
echo "du -skx (single thread)   median $(echo "$times" | median) s"

echo "--- memory (one scan, default settings)"
if [ "$(uname)" = Darwin ]; then
  nice -n 15 /usr/bin/time -l "$bin" "$target" 2>&1 >/dev/null | grep -E "maximum resident|peak memory"
else
  nice -n 15 /usr/bin/time -v "$bin" "$target" 2>&1 >/dev/null | grep -E "Maximum resident"
fi
echo "--- accuracy"
out=$(nice -n 15 "$bin" "$target")
echo "$out" | grep -E "files|folders|allocated|hard links|clones|sparse|unreadable|tree heap"
du_kib=$(nice -n 15 du -skx "$target" 2>/dev/null | awk '{print $1}' || true)
ours=$(echo "$out" | awk '/allocated KiB/ {print $3}')
saved=$(echo "$out" | awk '/^clones/ {print $2}')
echo "du -skx KiB    $du_kib"
awk -v o="$ours" -v d="$du_kib" -v s="$saved" 'BEGIN {
  printf "scan + clone savings - du = %d KiB (%.4f%% of du)\n", o + s - d, (o + s - d) * 100 / d;
  printf "scan - du                = %d KiB (clone savings are the expected difference)\n", o - d }'
