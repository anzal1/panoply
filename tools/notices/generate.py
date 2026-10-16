#!/usr/bin/env python3
"""Builds THIRD-PARTY-NOTICES.md from Cargo.lock, the Cargo.lock of every shipped community module
(modules/examples-wasm/*) and the hand-written parts in this folder.

    python3 -I tools/notices/generate.py            write THIRD-PARTY-NOTICES.md
    python3 -I tools/notices/generate.py --check    exit 1 if the file on disk is not what this would write

Needs the crate sources on disk, so run `cargo fetch --locked` first, and the same with
`--manifest-path modules/examples-wasm/<module>/Cargo.toml` for each module (CI does; see MODULE_FETCH).

What goes in: every package that the shipped programs can link, which is every package reachable from a
workspace member through normal (not dev, not build-only) dependencies, on every platform and with the
workspace's default features. Panoply's own crates are left out. The community modules in modules/examples-wasm ship inside the
macOS app and are their own Cargo projects, so each one's lock file is read the same way, for the
wasm32-wasip2 target the modules are built for. The vendored gpui-base is third-party code
and is listed. Each package contributes the license, notice and copying files found in its source folder.
Identical texts are printed once with the list of packages that carry them.

Parts written by hand:  header.md (credits and vendored code), runtime.md (downloads at run time).
`runtime.md` is checked against the pins in the source (see PIN_FILES): a changed release, commit or
version that is not mentioned there fails the check.
"""
import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(ROOT, "THIRD-PARTY-NOTICES.md")

# Packages that are Panoply's own, whatever their name.
OWN_REPO = "https://github.com/anzal1/panoply"

FILE_RE = re.compile(r"^(licen[cs]e|copying|unlicen[cs]e|notice|copyright|patents)", re.I)
SKIP_EXT = {".rs", ".toml", ".json", ".py", ".c", ".h", ".cc", ".cpp", ".in", ".sh", ".yml", ".yaml", ".lock"}
MAX_BYTES = 400_000

# Source files that pin what is downloaded at run time. runtime.md must mention every token found here.
PIN_FILES = [
    "crates/panoply-pdf/src/pdfium.rs",
    "crates/panoply-vision/src/models.rs",
    "crates/panoply-vision/src/runtime.rs",
    "crates/panoply-ocr/src/models.rs",
    "crates/panoply-id/src/runtime.rs",
    "crates/panoply-media/src/engine/pins.rs",
]


MODULES_DIR = os.path.join(ROOT, "modules", "examples-wasm")


def module_manifests():
    """Cargo.toml of every shipped community module that has a lock file."""
    if not os.path.isdir(MODULES_DIR):
        return []
    found = []
    for name in sorted(os.listdir(MODULES_DIR)):
        d = os.path.join(MODULES_DIR, name)
        if os.path.isfile(os.path.join(d, "Cargo.toml")) and os.path.isfile(os.path.join(d, "Cargo.lock")):
            found.append(os.path.join(d, "Cargo.toml"))
    return found


def run_metadata(manifest=None):
    cmd = ["cargo", "metadata", "--format-version", "1", "--locked"]
    if manifest:
        # Modules are built for WebAssembly only, so only what that target links is shipped.
        cmd += ["--manifest-path", manifest, "--filter-platform", "wasm32-wasip2"]
    out = subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def normalize(text):
    text = text.replace("\r\n", "\n").replace("\r", "\n").replace("\t", "    ")
    lines = [ln.rstrip() for ln in text.split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def shipped_packages(meta):
    byid = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    seen, stack = set(), list(meta["workspace_members"])
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] is None for k in d["dep_kinds"]):
                stack.append(d["pkg"])
    pkgs = []
    for i in seen:
        p = byid[i]
        if (p.get("repository") or "").rstrip("/").lower().startswith(OWN_REPO):
            continue
        if p["source"] is None and p["name"].startswith("panoply-"):
            continue
        pkgs.append(p)
    return sorted(pkgs, key=lambda p: (p["name"], p["version"]))


SKIP_DIRS = re.compile(r"^(tests?|examples?|benches|benchmarks?|fuzz|\.git|\.github|target|node_modules|docs?)$", re.I)


def license_files(pkg):
    """License, notice and copying files in the package folder, down to three levels (bundled C
    libraries keep theirs in vendor/ or a library folder)."""
    base = os.path.dirname(pkg["manifest_path"])
    found = []

    def take(path):
        name = os.path.basename(path)
        if os.path.splitext(name)[1].lower() in SKIP_EXT:
            return
        if os.path.getsize(path) > MAX_BYTES:
            return
        with open(path, "rb") as f:
            text = f.read().decode("utf-8", errors="replace")
        text = normalize(text)
        if text:
            found.append((os.path.relpath(path, base).replace(os.sep, "/"), text))

    def walk(d, depth, in_license_dir):
        for name in sorted(os.listdir(d)):
            path = os.path.join(d, name)
            if os.path.isfile(path):
                if FILE_RE.match(name) or in_license_dir:
                    take(path)
            elif os.path.isdir(path) and not os.path.islink(path):
                if SKIP_DIRS.match(name) or depth >= 3:
                    continue
                walk(path, depth + 1, in_license_dir or bool(re.fullmatch(r"licen[cs]es?", name, re.I)))

    walk(base, 0, False)
    return found


def fence(text):
    n = 4
    while ("`" * n) in text:
        n += 1
    return "`" * n


MIT_TEMPLATE = normalize("""
MIT License

Copyright (c) <the authors named above>

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and
to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of
the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO
THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
""")


def repo_root(p):
    r = (p.get("repository") or "").strip().lower().rstrip("/")
    r = re.sub(r"^https?://", "", r)
    r = re.sub(r"\.git$", "", r)
    parts = r.split("/")
    return "/".join(parts[:3]) if len(parts) >= 3 else ""


def spdx_ids(expr):
    return set(re.findall(r"[A-Za-z0-9.+-]+", (expr or "").replace("WITH", " ")))  - {"OR", "AND"}


def crates_section(pkgs):
    texts = {}  # sha -> [kind, text, [pkgs]]
    by_repo = {}  # repository root -> list of (spdx id set, text keys), from crates that do ship files
    bare = []

    def add(p, files):
        keys = []
        for rel, text in files:
            kind = "notice" if re.match(r"notice", os.path.basename(rel), re.I) else "license"
            key = hashlib.sha256(text.encode()).hexdigest()
            entry = texts.setdefault(key, [kind, text, []])
            label = f"{p['name']} {p['version']}"
            if label not in entry[2]:
                entry[2].append(label)
            keys.append(key)
        return keys

    for p in pkgs:
        files = license_files(p)
        if not files:
            bare.append(p)
            continue
        by_repo.setdefault(repo_root(p), []).append((spdx_ids(p.get("license")), add(p, files)))

    no_files, sibling = [], []
    for p in bare:
        root, ids = repo_root(p), spdx_ids(p.get("license"))
        keys = set()
        for sib_ids, sib_keys in by_repo.get(root, []) if root else []:
            if ids and ids <= sib_ids:
                keys.update(sib_keys)
        if keys:
            label = f"{p['name']} {p['version']}"
            for k in keys:
                if label not in texts[k][2]:
                    texts[k][2].append(label)
            sibling.append(p)
        else:
            no_files.append(p)

    lines = []
    lines.append("## 3. Rust crates\n")
    lines.append(
        f"{len(pkgs)} crates can be linked into the Panoply programs on one platform or another (the list is built "
        "from Cargo.lock and the lock files of the community modules by `tools/notices/generate.py`). For a crate offered under several licenses (for "
        "example MIT OR Apache-2.0), Panoply uses it under the permissive choice. The index comes first, "
        "then every distinct license and notice text once, with the crates that carry it.\n"
    )
    lines.append("### 3.1 Index\n")
    lines.append("| Crate | Version | License as declared |")
    lines.append("|---|---|---|")
    for p in pkgs:
        lic = (p.get("license") or "see its license file").replace("|", "\\|")
        lines.append(f"| {p['name']} | {p['version']} | {lic} |")
    lines.append("")
    if sibling:
        lines.append(
            f"{len(sibling)} crates publish no license file of their own. Their repository and declared license "
            "are the same as those of other crates in this list that do, and the texts below are listed against "
            "them as well.\n"
        )
    if no_files:
        lines.append("### 3.2 Crates that ship no license file\n")
        lines.append(
            "These crates declare their license in `Cargo.toml` but their published package has no license file "
            "and no sibling crate from the same repository carries one. The declared license applies; the authors "
            "are as the crate lists them.\n"
        )
        for p in no_files:
            authors = ", ".join(p.get("authors") or []) or "not listed"
            lines.append(f"- {p['name']} {p['version']}: {p.get('license') or 'no license field'}. Authors: {authors}.")
        lines.append("")
        lines.append(
            "For the MIT crates in this list the license is the standard MIT text, with the copyright holders "
            "being the authors named above (or the project named in the crate's repository):\n"
        )
        lines.append("````text")
        lines.append(MIT_TEMPLATE)
        lines.append("````\n")
        lines.append(
            "The Apache-2.0 crates in this list are under the Apache License 2.0, whose full text is printed in "
            "section 3.3. The crate that declares CC0-1.0 dedicates its work to the public domain "
            "(https://creativecommons.org/publicdomain/zero/1.0/legalcode).\n"
        )
    lines.append("### 3.3 License and notice texts\n")
    ordered = sorted(texts.values(), key=lambda e: (e[0] != "notice", -len(e[2]), e[2][0]))
    # Notices first (Apache-2.0 section 4(d)), then licenses, most used first.
    for n, (kind, text, users) in enumerate(ordered, 1):
        title = "Notice" if kind == "notice" else "License"
        lines.append(f"#### {title} text {n}\n")
        lines.append("Used by: " + ", ".join(sorted(users)) + "\n")
        f = fence(text)
        lines.append(f + "text")
        lines.append(text)
        lines.append(f + "\n")
    return "\n".join(lines), len(texts), no_files


def pin_tokens():
    tokens = set()
    for rel in PIN_FILES:
        with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
            src = f.read()
        for m in re.finditer(r'(?:ORT_VERSION|RELEASE|MODEL_COMMIT|VERSION)\s*:\s*&str\s*=\s*"([^"]+)"', src):
            tokens.add(m.group(1))
        for m in re.finditer(r'https://[^"\s]*?/([0-9a-f]{40})/', src):
            tokens.add(m.group(1)[:8])
        for m in re.finditer(r"autobuild-[0-9-]+", src):
            tokens.add(m.group(0))
    return sorted(tokens)


def read(name):
    with open(os.path.join(HERE, name), encoding="utf-8") as f:
        return f.read().strip("\n") + "\n"


def build():
    pkgs = shipped_packages(run_metadata())
    seen = {(p["name"], p["version"], p["source"]) for p in pkgs}
    for manifest in module_manifests():
        for p in shipped_packages(run_metadata(manifest)):
            key = (p["name"], p["version"], p["source"])
            if key not in seen:
                seen.add(key)
                pkgs.append(p)
    pkgs.sort(key=lambda p: (p["name"], p["version"]))
    section, ntexts, no_files = crates_section(pkgs)
    runtime = read("runtime.md")
    missing = [t for t in pin_tokens() if t not in runtime]
    if missing:
        sys.stderr.write(
            "tools/notices/runtime.md does not mention these pinned values from the source: "
            + ", ".join(missing)
            + "\nUpdate runtime.md (source URL and license for each changed download), then run the generator.\n"
        )
        sys.exit(1)
    doc = read("header.md") + "\n" + section + "\n" + runtime
    return doc, len(pkgs), ntexts, no_files


def main():
    check = "--check" in sys.argv
    doc, npk, ntexts, no_files = build()
    if check:
        try:
            with open(OUT, encoding="utf-8") as f:
                cur = f.read()
        except FileNotFoundError:
            cur = ""
        if cur != doc:
            sys.stderr.write(
                "THIRD-PARTY-NOTICES.md is out of date (a Cargo.lock or tools/notices changed).\n"
                "Run: cargo fetch --locked && for m in modules/examples-wasm/*/; do cargo fetch --locked --manifest-path $m/Cargo.toml; done && python3 -I tools/notices/generate.py\n"
                "and commit the result.\n"
            )
            sys.exit(1)
        print(f"THIRD-PARTY-NOTICES.md is current ({npk} crates).")
        return
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(doc)
    print(f"wrote {OUT}: {npk} crates, {ntexts} distinct texts, {len(no_files)} crates without a license file")
    for p in no_files:
        print("  no license file:", p["name"], p["version"], p.get("license"))


if __name__ == "__main__":
    main()
