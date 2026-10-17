#!/usr/bin/env python3
"""Checks that the Windows MSI ProductCode differs between a beta and its stable release.

The formula is the one in packaging/windows/package.ps1: MD5 of "panoply|ARCH|VERSION" read as a .NET Guid
(the first three groups little-endian), upper case. This test also reads package.ps1 and fails if it stops
hashing the full version string, so the two cannot drift apart unnoticed. Run with `python -I`.
"""
import hashlib
import re
import sys
import uuid
from pathlib import Path

PS1 = Path(__file__).resolve().parent.parent / "windows/package.ps1"


def product_code(arch, version):
    digest = hashlib.md5(f"panoply|{arch}|{version}".encode()).digest()
    return str(uuid.UUID(bytes_le=digest)).upper()


def main():
    text = PS1.read_text()
    m = re.search(r'GetBytes\("([^"]*)"\)', text)
    if not m or m.group(1) != "panoply|$Arch|$Version":
        sys.exit(f"package.ps1 no longer hashes panoply|$Arch|$Version (found {m.group(1) if m else None!r})")
    for arch in ("x64", "arm64"):
        beta, stable, beta2 = (product_code(arch, v) for v in ("0.2.0-beta", "0.2.0", "0.2.0-beta.2"))
        if len({beta, stable, beta2}) != 3:
            sys.exit(f"ProductCodes collide for {arch}: {beta} {stable} {beta2}")
        print(f"{arch}: 0.2.0-beta {{{beta}}}  0.2.0 {{{stable}}}  0.2.0-beta.2 {{{beta2}}}")
    if product_code("x64", "0.2.0") == product_code("arm64", "0.2.0"):
        sys.exit("x64 and arm64 share a ProductCode")
    print("ok: the ProductCode differs between a beta and its stable release")


if __name__ == "__main__":
    main()
