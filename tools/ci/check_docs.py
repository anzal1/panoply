#!/usr/bin/env python3
"""The docs check CI runs when only documentation changed (and on every push, because it takes a second).

    python3 -I tools/ci/check_docs.py

Every tracked Markdown file is read and every relative link and image in it must point at a file or
folder that exists. Vendored code under apps/desktop/patches/ is skipped. Links inside code blocks and inline code are ignored, and so are web links, mail
links and links that are only an anchor. Anchors inside other files are not checked.

Exit status 1 lists every broken link as file:line, the link and what it resolved to.
"""
import os
import re
import subprocess
import sys
from urllib.parse import unquote

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))

# Vendored third-party code: its Markdown links point into the project it came from.
SKIP_TREES = ("apps/desktop/patches/",)

# [text](target) and ![alt](target); the target may carry a title: [t](path "title") or <path with spaces>.
LINK = re.compile(r"!?\[(?:[^\]\\]|\\.)*\]\(\s*(<[^>]*>|[^)\s]+)(?:\s+(?:\"[^\"]*\"|'[^']*'))?\s*\)")
# Reference definitions: [id]: target
REFDEF = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*(<[^>]*>|\S+)")
FENCE = re.compile(r"^\s*(```+|~~~+)")
INLINE_CODE = re.compile(r"`+[^`]*`+")
SKIP_PREFIXES = ("http://", "https://", "mailto:", "tel:", "#", "data:", "ftp://")


def tracked_markdown():
    out = subprocess.check_output(["git", "ls-files", "-z", "--", "*.md"], cwd=ROOT)
    return [
        p
        for p in out.decode().split("\0")
        if p and not p.startswith(SKIP_TREES) and os.path.isfile(os.path.join(ROOT, p))
    ]


def links_in(text):
    in_fence = None
    for n, line in enumerate(text.splitlines(), 1):
        m = FENCE.match(line)
        if m:
            marker = m.group(1)[0]
            if in_fence is None:
                in_fence = marker
            elif in_fence == marker:
                in_fence = None
            continue
        if in_fence:
            continue
        bare = INLINE_CODE.sub("", line)
        for m in LINK.finditer(bare):
            yield n, m.group(1)
        m = REFDEF.match(bare)
        if m:
            yield n, m.group(1)


def main():
    problems = []
    files = tracked_markdown()
    for rel in files:
        with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
            text = f.read()
        base = os.path.dirname(os.path.join(ROOT, rel))
        for n, raw in links_in(text):
            target = raw.strip("<>")
            if not target or target.startswith(SKIP_PREFIXES) or "://" in target:
                continue
            path = unquote(target.split("#", 1)[0].split("?", 1)[0])
            if not path:
                continue
            full = os.path.normpath(os.path.join(ROOT, path.lstrip("/")) if path.startswith("/") else os.path.join(base, path))
            if not os.path.exists(full):
                problems.append(f"{rel}:{n}: {raw} -> {os.path.relpath(full, ROOT)} does not exist")
    if problems:
        print("\n".join(problems))
        print(f"\n{len(problems)} broken link(s) in {len(files)} Markdown files")
        return 1
    print(f"{len(files)} Markdown files, every relative link resolves")
    return 0


if __name__ == "__main__":
    sys.exit(main())
