#!/usr/bin/env python3
"""Make a .zip or .tar.gz from files, with one top-level folder and reproducible metadata.

    mkarchive.py OUT.(zip|tar.gz) TOPDIR SRC[=NAME] [SRC[=NAME] ...]

Each SRC is a file or a directory; NAME is its path inside TOPDIR (default: its basename). Entries
get a fixed timestamp (SOURCE_DATE_EPOCH or 2020-01-01), no owner, and mode 0755 for executables
(anything already executable, or ending in .exe, .sh, .AppImage) and 0644 otherwise.
Run it with `python -I` so nothing in the current directory can shadow the standard library.
"""
import gzip
import io
import os
import sys
import tarfile
import time
import zipfile
from pathlib import Path

EPOCH = int(os.environ.get("SOURCE_DATE_EPOCH", "1577836800"))


def is_exec(path: Path) -> bool:
    return os.access(path, os.X_OK) or path.suffix.lower() in {".exe", ".sh", ".appimage"}


def gather(args):
    """Yield (source Path, archive-relative name) for every file, directories expanded."""
    for spec in args:
        src, _, name = spec.partition("=")
        src_path = Path(src)
        name = name or src_path.name
        if src_path.is_dir():
            for root, dirs, files in os.walk(src_path):
                dirs.sort()
                for f in sorted(files):
                    full = Path(root) / f
                    yield full, f"{name}/{full.relative_to(src_path).as_posix()}"
        elif src_path.is_file():
            yield src_path, name
        else:
            sys.exit(f"mkarchive: missing input {src}")


def main():
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    out, top, *specs = sys.argv[1:]
    items = sorted(gather(specs), key=lambda it: it[1])
    out_path = Path(out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    if out.endswith(".zip"):
        with zipfile.ZipFile(out_path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
            for src, name in items:
                info = zipfile.ZipInfo(f"{top}/{name}", time.gmtime(EPOCH)[:6])
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (0o755 if is_exec(src) else 0o644) << 16
                z.writestr(info, src.read_bytes())
    elif out.endswith(".tar.gz"):
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as t:
            for src, name in items:
                info = tarfile.TarInfo(f"{top}/{name}")
                data = src.read_bytes()
                info.size = len(data)
                info.mtime = EPOCH
                info.mode = 0o755 if is_exec(src) else 0o644
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                t.addfile(info, io.BytesIO(data))
        with open(out_path, "wb") as f, gzip.GzipFile(fileobj=f, mode="wb", mtime=EPOCH, compresslevel=9) as g:
            g.write(raw.getvalue())
    else:
        sys.exit("mkarchive: output must end in .zip or .tar.gz")
    print(f"wrote {out_path} ({out_path.stat().st_size} bytes, {len(items)} files)")


if __name__ == "__main__":
    main()
