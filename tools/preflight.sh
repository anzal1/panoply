#!/usr/bin/env bash
# A local replica of the CI checks. Run it before every push.
#
#     tools/preflight.sh            fmt, clippy and tests for the crates changed since origin/main and
#                                   every crate that depends on them
#     tools/preflight.sh --all      the whole workspace
#     tools/preflight.sh --heavy    also the heavy corpus and model tests (CI runs those on Linux only)
#     tools/preflight.sh --plan     only print what would run
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
# the docs check. It runs at low priority (nice -n 10) so the machine stays usable, and builds into
# $CARGO_TARGET_DIR, which defaults to a folder whose name ends in .noindex (Spotlight skips those):
# ~/personal/panoply-shared.noindex if it exists, else ~/panoply-target.noindex. A warm folder makes a
# typical run 1 to 3 minutes.
set -euo pipefail

if [ -z "${PANOPLY_PREFLIGHT_NICE:-}" ]; then
  PANOPLY_PREFLIGHT_NICE=1 exec nice -n 10 "$0" "$@"
fi

all=0
heavy=0
plan_only=0
for a in "$@"; do
  case "$a" in
    --all) all=1 ;;
    --heavy) heavy=1 ;;
    --plan) plan_only=1 ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | grep '^#' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $a (see --help)" >&2; exit 2 ;;
  esac
done

cd "$(git rev-parse --show-toplevel)"

if [ -z "${CARGO_TARGET_DIR:-}" ]; then
  if [ -d "$HOME/personal/panoply-shared.noindex" ]; then
    CARGO_TARGET_DIR="$HOME/personal/panoply-shared.noindex"
  else
    CARGO_TARGET_DIR="$HOME/panoply-target.noindex"
  fi
  export CARGO_TARGET_DIR
fi
export CARGO_INCREMENTAL=0 PANOPLY_YES=1 CARGO_TERM_COLOR=always
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
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

# Writes "all" or a list of "-p crate" words, or nothing when no Rust crate is touched.
plan="$(
  printf '%s\n' "$changed" | ALL="$all" python3 -I -c '
import json, os, subprocess, sys

files = [l for l in sys.stdin.read().split("\n") if l]
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"]))
root = meta["workspace_root"].rstrip("/") + "/"
pkgs = {p["name"]: os.path.dirname(p["manifest_path"]).removeprefix(root) for p in meta["packages"]}

FULL = ("Cargo.lock", "Cargo.toml", "rust-toolchain.toml", "rustfmt.toml", "deny.toml", ".config/nextest.toml")
if os.environ["ALL"] == "1" or any(f in FULL or f.startswith(".github/workflows/") for f in files):
    print("all"); sys.exit(0)

hit = set()
for f in files:
    best = None
    for name, d in pkgs.items():
        if f == d or f.startswith(d + "/"):
            if best is None or len(d) > len(pkgs[best]):
                best = name
    if best:
        hit.add(best)
# Every crate that depends on a changed crate, however far up.
users = {n: set() for n in pkgs}
for p in meta["packages"]:
    for dep in p["dependencies"]:
        if dep["name"] in pkgs:
            users[dep["name"]].add(p["name"])
todo = list(hit)
while todo:
    for u in users[todo.pop()]:
        if u not in hit:
            hit.add(u)
            todo.append(u)
print(" ".join(f"-p {n}" for n in sorted(hit)))
'
)"

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

if [ "$plan_only" = 1 ]; then
  exit 0
fi

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

printf '\npreflight passed in %ss\n' "$((SECONDS - start))"
