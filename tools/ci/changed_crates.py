"""Which crates does a list of changed files touch? Shared by tools/preflight.sh and tools/preflight-linux.sh.

Reads changed file names (one per line) on stdin and prints one line:

    all                  the whole workspace is in scope (ALL=1, or a root manifest, lockfile, workflow
                         or nextest config changed)
    -p a -p b            the changed crates and every crate that depends on them, however far up
    (nothing)            no Rust crate is touched

Run it from inside the workspace: it calls `cargo metadata`.
"""

import json
import os
import subprocess
import sys

files = [line for line in sys.stdin.read().split("\n") if line]
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"]))
root = meta["workspace_root"].rstrip("/") + "/"
pkgs = {p["name"]: os.path.dirname(p["manifest_path"]).removeprefix(root) for p in meta["packages"]}

FULL = ("Cargo.lock", "Cargo.toml", "rust-toolchain.toml", "rustfmt.toml", "deny.toml", ".config/nextest.toml")
if os.environ.get("ALL") == "1" or any(f in FULL or f.startswith(".github/workflows/") for f in files):
    print("all")
    sys.exit(0)

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
