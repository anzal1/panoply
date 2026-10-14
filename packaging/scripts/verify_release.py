#!/usr/bin/env python3
"""Fail unless a release folder holds every promised artifact and the archives carry the notices and icon.

    verify_release.py --version 0.1.0-beta --dir release

Required (V = version): Panoply-V-macos-{arm64,x64}.{dmg,zip}, Panoply-V-windows-{x64,arm64}.{zip,msi},
Panoply-V-linux-{x64,arm64}.{tar.gz,AppImage}. Every desktop .zip and .tar.gz must contain
THIRD-PARTY-NOTICES.md and an app icon (.ico, .icns or .png). The .dmg, .msi and .AppImage are checked
where they are built (macos-package.sh, package.ps1, linux-package.sh). Run with `python -I`.
"""
import argparse
import sys
import tarfile
import zipfile
from pathlib import Path

ICON_SUFFIXES = (".ico", ".icns", ".png")


def names(path: Path):
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as z:
            return z.namelist()
    with tarfile.open(path, "r:gz") as t:
        return t.getnames()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", required=True)
    ap.add_argument("--dir", required=True)
    a = ap.parse_args()
    d, v = Path(a.dir), a.version
    required = (
        [f"Panoply-{v}-macos-{x}.{e}" for x in ("arm64", "x64") for e in ("dmg", "zip")]
        + [f"Panoply-{v}-windows-{x}.{e}" for x in ("x64", "arm64") for e in ("zip", "msi")]
        + [f"Panoply-{v}-linux-{x}.{e}" for x in ("x64", "arm64") for e in ("tar.gz", "AppImage")]
    )
    problems = [f"missing {f}" for f in required if not (d / f).is_file()]
    for f in required:
        p = d / f
        if not p.is_file():
            continue
        if p.name.endswith((".zip", ".tar.gz")):
            n = names(p)
            if not any(x.endswith("/THIRD-PARTY-NOTICES.md") or x.endswith("/Resources/THIRD-PARTY-NOTICES.md") for x in n):
                problems.append(f"{f}: no THIRD-PARTY-NOTICES.md")
            if not any(x.lower().endswith(ICON_SUFFIXES) for x in n):
                problems.append(f"{f}: no app icon")
        print(f"ok  {f}  {p.stat().st_size} bytes" if not any(f in q for q in problems) else f"BAD {f}")
    if problems:
        sys.exit("verify_release: " + "; ".join(problems))
    print("every required artifact is present, with notices and icon")


if __name__ == "__main__":
    main()
