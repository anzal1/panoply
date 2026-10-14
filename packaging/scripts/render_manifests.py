#!/usr/bin/env python3
"""Fill the package manager templates under packaging/ from a finished release.

    render_manifests.py --version 0.1.0 --repo anzal1/panoply --sums SHA256SUMS --out DIR
                        [--commit SHA] [--macos-notarized 0|1] [--state STATE_DIR]

Placeholders are @NAME@. @VERSION@, @REPO@, @COMMIT@, @RELEASE_DATE@ come from the arguments;
@SHA256_<KEY>@ is looked up in SHA256SUMS by the release asset name (table ASSETS below);
@PRODUCT_CODE_<ARCH>@ comes from state/windows-<arch>.env (written by packaging/windows/package.ps1);
@MACOS_CAVEATS@ is a Homebrew caveats block, empty once the app is notarized.
An asset missing from the release is filled with 64 zeros and reported as a GitHub warning, so a
partial release still renders and the gap is visible. Run with `python -I`.
"""
import argparse
import datetime
import json
import re
import sys
from pathlib import Path

PKG = Path(__file__).resolve().parent.parent

# key -> release asset name
ASSETS = {
    "MACOS_ZIP": "Panoply-{v}-macos-universal.zip",
    "WIN_X64_ZIP": "Panoply-{v}-windows-x64.zip",
    "WIN_ARM64_ZIP": "Panoply-{v}-windows-arm64.zip",
    "WIN_X64_MSI": "Panoply-{v}-windows-x64.msi",
    "WIN_ARM64_MSI": "Panoply-{v}-windows-arm64.msi",
}

# template -> output path (relative to --out)
OUTPUTS = {
    "homebrew/panoply.rb.in": "homebrew/panoply.rb",
    "scoop/panoply.json.in": "scoop/panoply.json",
    "winget/anzal1.Panoply.yaml.in": "winget/{v}/anzal1.Panoply.yaml",
    "winget/anzal1.Panoply.installer.yaml.in": "winget/{v}/anzal1.Panoply.installer.yaml",
    "winget/anzal1.Panoply.locale.en-US.yaml.in": "winget/{v}/anzal1.Panoply.locale.en-US.yaml",
    "flatpak/io.github.anzal1.Panoply.yml.in": "flatpak/io.github.anzal1.Panoply.yml",
    "flatpak/io.github.anzal1.Panoply.metainfo.xml.in": "flatpak/io.github.anzal1.Panoply.metainfo.xml",
}

CAVEATS = '''
  caveats <<~EOS
    This build is not notarized yet, so macOS may stop the first launch.
    Allow it in System Settings > Privacy & Security > Open Anyway, or run:
      xattr -dr com.apple.quarantine /Applications/Panoply.app
  EOS
'''


def read_sums(path):
    sums = {}
    for line in Path(path).read_text().splitlines():
        m = re.match(r"^([0-9a-fA-F]{64})\s+\*?(.+)$", line.strip())
        if m:
            sums[m.group(2).strip()] = m.group(1).lower()
    return sums


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", required=True)
    ap.add_argument("--repo", required=True)
    ap.add_argument("--sums", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--commit", default="0" * 40)
    ap.add_argument("--macos-notarized", default="0")
    ap.add_argument("--state", default=None)
    a = ap.parse_args()

    sums = read_sums(a.sums)
    subs = {
        "VERSION": a.version,
        "REPO": a.repo,
        "COMMIT": a.commit,
        "RELEASE_DATE": datetime.date.today().isoformat(),
        "MACOS_CAVEATS": "" if a.macos_notarized == "1" else CAVEATS,
    }
    missing = []
    for key, name in ASSETS.items():
        name = name.format(v=a.version)
        if name in sums:
            subs[f"SHA256_{key}"] = sums[name]
        else:
            subs[f"SHA256_{key}"] = "0" * 64
            missing.append(name)
    for arch in ("x64", "arm64"):
        code = "{00000000-0000-0000-0000-000000000000}"
        env = Path(a.state or "") / f"windows-{arch}.env"
        if a.state and env.is_file():
            for line in env.read_text().splitlines():
                if line.startswith("PRODUCT_CODE="):
                    code = line.split("=", 1)[1].strip()
        else:
            missing.append(f"ProductCode for windows-{arch}")
        subs[f"PRODUCT_CODE_{arch.upper()}"] = code

    for m in missing:
        print(f"::warning title=Manifest placeholder::{m} is not in this release; its manifest field is a placeholder.")

    out = Path(a.out)
    for tpl, dest in OUTPUTS.items():
        text = (PKG / tpl).read_text()
        for k, v in subs.items():
            text = text.replace(f"@{k}@", v)
        left = re.findall(r"@[A-Z0-9_]+@", text)
        if left:
            sys.exit(f"render_manifests: unfilled placeholders in {tpl}: {sorted(set(left))}")
        target = out / dest.format(v=a.version)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
        print(f"wrote {target}")
    json.loads((out / "scoop/panoply.json").read_text())  # the Scoop manifest must stay valid JSON


if __name__ == "__main__":
    main()
