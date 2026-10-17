"""Which crates does a list of changed files touch? Shared by tools/preflight.sh and tools/preflight-linux.sh.

Reads changed file names (one per line) on stdin and prints one line:

    all                  the whole workspace is in scope (ALL=1, or a root manifest, lockfile, workflow
                         or nextest config changed; a workflow change that stays inside the `docs` job
                         does not count, see only_docs_job_changed)
    -p a -p b            the changed crates and every crate that depends on them, however far up
    (nothing)            no Rust crate is touched

Run it from inside the workspace: it calls `cargo metadata`.
"""

import json
import os
import re
import subprocess
import sys

files = [line for line in sys.stdin.read().split("\n") if line]
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"]))
root = meta["workspace_root"].rstrip("/") + "/"
pkgs = {p["name"]: os.path.dirname(p["manifest_path"]).removeprefix(root) for p in meta["packages"]}


def strip_docs_job(text):
    """The workflow text without the `docs` job (and the comment lines just above it). None when it has no jobs."""
    lines = text.split("\n")
    out, skipping, pending = [], False, []
    for line in lines:
        header = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if header or (not line.startswith("  ") and line.strip() and not line.startswith("#")):
            # a new job (or a new top level key) ends the docs job; held comments belong to what follows
            skipping = bool(header) and header.group(1) == "docs"
            if not skipping:
                out.extend(pending)
            pending = []
            if not skipping:
                out.append(line)
        elif skipping:
            continue
        elif line.startswith("  #") or (not line.strip() and pending):
            pending.append(line)
        else:
            out.extend(pending)
            pending = []
            out.append(line)
    out.extend(pending)
    return "\n".join(out) if "jobs:" in text else None


def only_docs_job_changed(path):
    """True when the workflow file differs from origin/main only inside its `docs` job. Any doubt: False."""
    try:
        base = subprocess.check_output(["git", "merge-base", "origin/main", "HEAD"], text=True, stderr=subprocess.DEVNULL).strip()
        old = subprocess.check_output(["git", "show", f"{base}:{path}"], text=True, stderr=subprocess.DEVNULL)
        with open(path, encoding="utf-8") as f:
            new = f.read()
    except (subprocess.CalledProcessError, OSError):
        return False
    a, b = strip_docs_job(old), strip_docs_job(new)
    return a is not None and a == b


def full_scope(f):
    if f in FULL:
        return True
    return f.startswith(".github/workflows/") and not (f.endswith("/ci.yml") and only_docs_job_changed(f))


FULL = ("Cargo.lock", "Cargo.toml", "rust-toolchain.toml", "rustfmt.toml", "deny.toml", ".config/nextest.toml")
if os.environ.get("ALL") == "1" or any(full_scope(f) for f in files):
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
