#!/usr/bin/env bash
# A local replica of the CI checks. Run it before every push.
#
#     tools/preflight.sh            fmt, clippy and tests for the crates changed since origin/main and
#                                   every crate that depends on them
#     tools/preflight.sh --all      the whole workspace
#     tools/preflight.sh --heavy    also the heavy corpus and model tests (CI runs those on Linux only)
#     tools/preflight.sh --plan     only print what would run
#     tools/preflight.sh --linux    also the Linux check in a local VM (tools/preflight-linux.sh); it runs by
#                                   itself when a changed file holds Linux-only code (cfg(target_os = "linux"),
#                                   or src/platform/other.rs). --no-linux skips it.
#
# What it runs, the same way CI does (docs/CI.md):
#   cargo fmt --all --check
#   cargo clippy -p <crates> --all-targets -- -D warnings
#   cargo nextest run -p <crates>, with the `ci-core` profile (everything except the heavy tests), then
#   the timing tests alone (profile `ci-idle`)
#   python3 tools/ci/check_docs.py
#
# The whole workspace is used instead of the changed crates when Cargo.lock, a Cargo.toml in the root,
# a workflow or .config/nextest.toml changed, or when --all is given. Documentation-only changes run only
# the docs check. It runs at low priority (nice -n 15, 8 cargo jobs) and only one heavy check runs on the
# Mac at a time (tools/lib/heavy-lock.sh: a second one waits), so the machine stays usable, and builds into
# $CARGO_TARGET_DIR, which defaults to a folder whose name ends in .noindex (Spotlight skips those):
# ~/personal/panoply-shared.noindex if it exists, else ~/panoply-target.noindex. A warm folder makes a
# typical run 1 to 3 minutes.
set -euo pipefail

if [ -z "${PANOPLY_PREFLIGHT_NICE:-}" ]; then
  PANOPLY_PREFLIGHT_NICE=1 exec nice -n 15 "$0" "$@"
fi

all=0
heavy=0
plan_only=0
linux=auto
for a in "$@"; do
  case "$a" in
    --all) all=1 ;;
    --heavy) heavy=1 ;;
    --plan) plan_only=1 ;;
    --linux) linux=1 ;;
    --no-linux) linux=0 ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | grep '^#' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $a (see --help)" >&2; exit 2 ;;
  esac
done

cd "$(git rev-parse --show-toplevel)"
# shellcheck source=lib/heavy-lock.sh
. tools/lib/heavy-lock.sh

if [ -z "${CARGO_TARGET_DIR:-}" ]; then
  if [ -d "$HOME/personal/panoply-shared.noindex" ]; then
    CARGO_TARGET_DIR="$HOME/personal/panoply-shared.noindex"
  else
    CARGO_TARGET_DIR="$HOME/panoply-target.noindex"
  fi
  export CARGO_TARGET_DIR
fi
export CARGO_INCREMENTAL=0 PANOPLY_YES=1 CARGO_TERM_COLOR=always
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"; heavy_lock_release' EXIT
export PANOPLY_DATA_DIR="$scratch/data"

start=$SECONDS
step() { printf '\n==> %s\n' "$*"; step_start=$SECONDS; }
done_step() { printf '    %ss\n' "$((SECONDS - step_start))"; }

# ---- what changed -------------------------------------------------------------------------------
base="$(git merge-base origin/main HEAD 2>/dev/null || echo HEAD)"
changed="$({ git diff --name-only "$base"; git ls-files --others --exclude-standard; } | sort -u)"
if [ -z "$changed" ]; then
  echo "nothing has changed since origin/main"
fi

step "docs check"
python3 -I tools/ci/check_docs.py
done_step

# ---- the test corpus -----------------------------------------------------------------------------
# Corpus tests fail (they never skip quietly) when the files are missing. A new git worktree starts
# without them: link the cache of the first worktree when it has one, else fetch what CI fetches.
if [ ! -e tests/corpus/cache ]; then
  first="$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')"
  if [ -d "$first/tests/corpus/cache" ] && [ "$first" != "$PWD" ]; then
    ln -s "$first/tests/corpus/cache" tests/corpus/cache
    echo "corpus: linked $first/tests/corpus/cache"
  else
    step "fetching the test corpus (once)"
    python3 -I tests/corpus/fetch.py --kind pdf,image
    python3 tests/corpus/generate.py --kind pdf,image || echo "some generated files need local tools; their tests skip"
    done_step
  fi
fi

# Writes "all" or a list of "-p crate" words, or nothing when no Rust crate is touched.
plan="$(printf '%s\n' "$changed" | ALL="$all" python3 -I tools/ci/changed_crates.py)"

if [ -z "$plan" ]; then
  echo
  echo "No Rust crate changed: the docs check was all that applies."
  echo "preflight passed in $((SECONDS - start))s"
  exit 0
fi

if [ "$plan" = all ]; then
  scope="--workspace --exclude gpui-base"
  echo "scope: the whole workspace"
else
  scope="$plan"
  echo "scope: $plan"
fi

# Linux-only code among the changed files? Then the Linux check runs too (unless --no-linux).
# gpui-base is a vendored copy that is not linted, so it does not count.
linux_code="$(printf '%s\n' "$changed" | grep -E '\.rs$' | grep -v '^apps/desktop/patches/' | while read -r f; do
  [ -f "$f" ] || continue
  case "$f" in */platform/other.rs) echo "$f"; continue ;; esac
  grep -qE 'target_os *= *"linux"' "$f" && echo "$f"
done || true)"
if [ "$linux" = auto ]; then
  if [ -n "$linux_code" ]; then
    linux=1
    echo "linux: Linux-only code changed ($(printf '%s' "$linux_code" | head -1)$([ "$(printf '%s\n' "$linux_code" | wc -l)" -gt 1 ] && echo ' and more')), the Linux check will run"
  else
    linux=0
  fi
fi

if [ "$plan_only" = 1 ]; then
  [ "$linux" = 1 ] && echo "linux: tools/preflight-linux.sh would run after the macOS checks"
  exit 0
fi

# One heavy check at a time on this Mac; waits while another worktree or agent is building.
heavy_lock_acquire

step "cargo fmt --all --check"
cargo fmt --all --check
done_step

step "cargo clippy (-D warnings)"
# shellcheck disable=SC2086
cargo clippy $scope --all-targets -- -D warnings
done_step

profile=ci-core
[ "$heavy" = 1 ] && profile=ci

step "cargo nextest run (profile $profile)"
# shellcheck disable=SC2086
cargo nextest run $scope --profile "$profile" --lib --bins --tests --no-fail-fast --no-tests=pass
done_step

step "timing tests, alone (profile ci-idle)"
# shellcheck disable=SC2086
cargo nextest run $scope --profile ci-idle --lib --bins --tests --no-fail-fast --no-tests=pass
done_step

printf '\nmacOS checks passed in %ss\n' "$((SECONDS - start))"

if [ "$linux" = 1 ]; then
  step "Linux check (tools/preflight-linux.sh)"
  lflags=()
  [ "$all" = 1 ] && lflags+=(--all)
  [ "$heavy" = 1 ] && lflags+=(--heavy)
  # This script holds the heavy lock; the Linux script sees PANOPLY_HEAVY_LOCK_HELD and does not wait for it.
  tools/preflight-linux.sh ${lflags[@]+"${lflags[@]}"}
fi

printf '\npreflight passed in %ss\n' "$((SECONDS - start))"
