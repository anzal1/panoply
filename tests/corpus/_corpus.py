"""Shared helpers for fetch.py and generate.py.

Reads the per-kind manifests (pdf.toml, image.toml, ...) and rewrites hashes in place.
Standard library only, and it runs on Python 3.9 and newer. The manifests are real TOML
(the Rust harness reads them with the `toml` crate). This module parses only the subset
the manifests use: a header of comments, then one `[[file]]` table per file with string,
integer, boolean and one-line string-array values.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, "cache")
KINDS = ("pdf", "image", "video", "audio", "office")

# Licenses that ask more of a redistributor than attribution. `fetch.py --permissive-only`
# skips them, so a corpus copy can be bundled into a permissively licensed artifact.
NON_PERMISSIVE_PREFIXES = ("CC-BY-SA", "CC-BY-NC", "CC-BY-ND", "LGPL", "GPL", "AGPL")

REQUIRED = ("id", "file", "license", "source", "why")


def is_permissive(license_id: str) -> bool:
    return not license_id.upper().startswith(NON_PERMISSIVE_PREFIXES)


def manifest_path(kind: str) -> str:
    return os.path.join(HERE, kind + ".toml")


def cache_path(kind: str, entry: dict) -> str:
    return os.path.join(CACHE, kind, entry["file"])


def _parse_value(raw: str, where: str):
    raw = raw.strip()
    if raw.startswith('"'):
        # Basic strings: the JSON escape set is the same subset the manifests use.
        end = _string_end(raw)
        if end != len(raw) - 1 and raw[end + 1:].strip() and not raw[end + 1:].strip().startswith("#"):
            raise ValueError(f"{where}: junk after string")
        return json.loads(raw[: end + 1])
    if raw.startswith("["):
        close = raw.rfind("]")
        return json.loads(raw[: close + 1])
    token = raw.split("#", 1)[0].strip()
    if token in ("true", "false"):
        return token == "true"
    if re.fullmatch(r"-?\d+", token.replace("_", "")):
        return int(token.replace("_", ""))
    raise ValueError(f"{where}: unsupported value {raw!r}")


def _string_end(raw: str) -> int:
    i = 1
    while i < len(raw):
        if raw[i] == "\\":
            i += 2
            continue
        if raw[i] == '"':
            return i
        i += 1
    raise ValueError("unterminated string")


def load_manifest(kind: str) -> list[dict]:
    """Return the `[[file]]` tables of one manifest, each tagged with its `kind`."""
    path = manifest_path(kind)
    entries: list[dict] = []
    cur: dict | None = None
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            s = line.strip()
            if not s or s.startswith("#"):
                continue
            if s == "[[file]]":
                cur = {"kind": kind}
                entries.append(cur)
                continue
            if cur is None:
                raise ValueError(f"{path}:{n}: key outside a [[file]] table")
            key, _, val = s.partition("=")
            cur[key.strip()] = _parse_value(val, f"{path}:{n}")
    for e in entries:
        for k in REQUIRED:
            if k not in e:
                raise ValueError(f"{path}: entry {e.get('id', '?')} is missing {k!r}")
        e.setdefault("tags", [])
        e.setdefault("sha256", "")
        e.setdefault("size", 0)
        e.setdefault("tier", "default")
    ids = [e["id"] for e in entries]
    dupes = {i for i in ids if ids.count(i) > 1}
    if dupes:
        raise ValueError(f"{path}: duplicate ids {sorted(dupes)}")
    files = [e["file"] for e in entries]
    if len(set(files)) != len(files):
        raise ValueError(f"{path}: duplicate file names")
    return entries


def load_all(kinds=KINDS) -> list[dict]:
    out: list[dict] = []
    for k in kinds:
        out.extend(load_manifest(k))
    return out


def select(entries, ids=None, tags=None, permissive_only=False, tiers=("default",)):
    out = []
    for e in entries:
        if ids and e["id"] not in ids:
            continue
        if tags and not set(tags) & set(e["tags"]):
            continue
        if permissive_only and not is_permissive(e["license"]):
            continue
        if e["tier"] not in tiers and "all" not in tiers:
            continue
        out.append(e)
    return out


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            chunk = f.read(1 << 20)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


_fill_lock = threading.Lock()


def fill_hash(kind: str, entry_id: str, sha256: str, size: int) -> None:
    """Rewrite `sha256` and `size` for one entry in its manifest, keeping everything else.

    Safe to call from several threads: the read-modify-write is serialized and the file is
    replaced atomically.
    """
    with _fill_lock:
        _fill_hash_locked(kind, entry_id, sha256, size)


def _fill_hash_locked(kind: str, entry_id: str, sha256: str, size: int) -> None:
    path = manifest_path(kind)
    with open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    in_block = False
    done = {"sha256": False, "size": False}
    for i, line in enumerate(lines):
        s = line.strip()
        if s == "[[file]]":
            in_block = False
            continue
        if s.startswith("id") and s.split("=", 1)[0].strip() == "id":
            in_block = json.loads(s.split("=", 1)[1].strip()) == entry_id
            continue
        if in_block and s.split("=", 1)[0].strip() == "sha256":
            lines[i] = f'sha256 = "{sha256}"'
            done["sha256"] = True
        elif in_block and s.split("=", 1)[0].strip() == "size":
            lines[i] = f"size = {size}"
            done["size"] = True
    if not all(done.values()):
        raise ValueError(f"{path}: entry {entry_id} needs sha256 and size lines to fill")
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    os.replace(tmp, path)


def human(n: int) -> str:
    x = float(n)
    for unit in ("B", "KB", "MB", "GB"):
        if x < 1024 or unit == "GB":
            return f"{x:.0f} {unit}" if unit == "B" else f"{x:.1f} {unit}"
        x /= 1024
    return f"{n} B"


def die(msg: str, code: int = 2):
    print(msg, file=sys.stderr)
    sys.exit(code)
