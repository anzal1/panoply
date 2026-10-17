#!/usr/bin/env python3
"""Fill the package manager templates under packaging/ from a finished release.

    render_manifests.py --version 0.1.0 --repo anzal1/panoply --sums SHA256SUMS --out DIR
                        [--commit SHA] [--macos-notarized 0|1] [--state STATE_DIR]
    render_manifests.py --tag v0.2.0-beta [--repo anzal1/panoply] --out DIR     (needs the gh CLI)
                        [--msi-version 0.2.0.0]   (default: derived from the version like release.yml's plan job)

The second form renders every channel for a published release in one run: it downloads SHA256SUMS and the
release's own panoply-package-manifests zip with gh (the zip holds the MSI ProductCodes the Windows jobs
recorded), and takes the release date from the release. Channels: Homebrew, Scoop, winget, Flatpak, AUR.

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
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

PKG = Path(__file__).resolve().parent.parent

# key -> release asset name
ASSETS = {
    "MACOS_ARM64_ZIP": "Panoply-{v}-macos-arm64.zip",
    "MACOS_X64_ZIP": "Panoply-{v}-macos-x64.zip",
    "WIN_X64_ZIP": "Panoply-{v}-windows-x64.zip",
    "WIN_ARM64_ZIP": "Panoply-{v}-windows-arm64.zip",
    "WIN_X64_MSI": "Panoply-{v}-windows-x64.msi",
    "WIN_ARM64_MSI": "Panoply-{v}-windows-arm64.msi",
    "LINUX_X64_TGZ": "Panoply-{v}-linux-x64.tar.gz",
    "LINUX_ARM64_TGZ": "Panoply-{v}-linux-arm64.tar.gz",
}

# template -> output path (relative to --out)
OUTPUTS = {
    "homebrew/panoply.rb.in": "homebrew/panoply.rb",
    "scoop/panoply.json.in": "scoop/panoply.json",
    "winget/anzal1.Panoply.yaml.in": "winget/{v}/anzal1.Panoply.yaml",
    "winget/anzal1.Panoply.installer.yaml.in": "winget/{v}/anzal1.Panoply.installer.yaml",
    "winget/anzal1.Panoply.locale.en-US.yaml.in": "winget/{v}/anzal1.Panoply.locale.en-US.yaml",
    "flatpak/io.github.anzal1.Panoply.yml.in": "flatpak/io.github.anzal1.Panoply.yml",
    "flatpak/io.github.anzal1.Panoply.sideload.yml.in": "flatpak/io.github.anzal1.Panoply.sideload.yml",
    "flatpak/io.github.anzal1.Panoply.metainfo.xml.in": "flatpak/io.github.anzal1.Panoply.metainfo.xml",
    "aur/PKGBUILD.in": "aur/PKGBUILD",
    "aur/SRCINFO.in": "aur/.SRCINFO",
}

CAVEATS = '''
  caveats <<~EOS
    Panoply is not signed or notarized (no Apple Developer account yet), so macOS blocks the first launch.
    Allow it once in System Settings > Privacy & Security > Open Anyway, or run:
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


ZERO_GUID = "{00000000-0000-0000-0000-000000000000}"


def msi_version(version):
    """The MSI version for a release version, the way the `plan` job in release.yml derives it:
    the numeric base plus the trailing number of the prerelease (0.2.0 -> 0.2.0.0, 0.2.0-beta.3 -> 0.2.0.3)."""
    base, _, pre = version.partition("-")
    m = re.search(r"([0-9]*)$", pre)
    return f"{base}.{(m.group(1) if m else '') or 0}"


def gh(*args):
    try:
        return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout
    except FileNotFoundError:
        sys.exit("render_manifests: --tag needs the gh CLI (https://cli.github.com), which is not installed. "
                 "Pass --version and --sums instead.")
    except subprocess.CalledProcessError as e:
        sys.exit(f"render_manifests: gh {' '.join(args[:2])} failed: {(e.stderr or '').strip()}")


def fetch_release(tag, repo, work):
    """Download SHA256SUMS and the release's manifests zip; return (sums path, state dir, release date)."""
    gh("release", "download", tag, "-R", repo, "-p", "SHA256SUMS", "-p", "panoply-package-manifests-*.zip",
       "-D", str(work), "--clobber")
    state = work / "state"
    state.mkdir()
    for z in work.glob("panoply-package-manifests-*.zip"):
        with zipfile.ZipFile(z) as zf:
            for name in zf.namelist():
                if name.endswith("anzal1.Panoply.installer.yaml"):
                    text = zf.read(name).decode()
                    # Installers list x64 first, then arm64, each followed by its ProductCode.
                    for arch, code in re.findall(r"Architecture: (\w+)\n(?:.*\n)*?\s+ProductCode: '([^']+)'", text):
                        (state / f"windows-{arch}.env").write_text(f"PRODUCT_CODE={code}\n")
    info = json.loads(gh("release", "view", tag, "-R", repo, "--json", "publishedAt,createdAt"))
    date = (info.get("publishedAt") or info.get("createdAt") or "")[:10]
    return work / "SHA256SUMS", state, date


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--version")
    ap.add_argument("--tag", help="render a published release: the version, sums, ProductCodes and date come from gh")
    ap.add_argument("--msi-version", default=None, help="MSI ProductVersion (release.yml plan.msi_version)")
    ap.add_argument("--repo", default="anzal1/panoply")
    ap.add_argument("--sums")
    ap.add_argument("--out", required=True)
    ap.add_argument("--commit", default="0" * 40)
    ap.add_argument("--macos-notarized", default="0")
    ap.add_argument("--state", default=None)
    a = ap.parse_args()

    workdirs = []
    try:
        _render(a, workdirs)
    finally:
        for d in workdirs:
            shutil.rmtree(d, ignore_errors=True)


def _render(a, workdirs):
    release_date = datetime.date.today().isoformat()
    if a.tag:
        a.version = a.version or a.tag.removeprefix("v")
        work = Path(tempfile.mkdtemp(prefix="panoply-render-"))
        workdirs.append(work)
        sums_path, state, date = fetch_release(a.tag, a.repo, work)
        a.sums = a.sums or str(sums_path)
        a.state = a.state or str(state)
        release_date = date or release_date
        if a.commit == "0" * 40:  # the Flatpak source manifest pins the tagged commit
            a.commit = gh("api", f"repos/{a.repo}/commits/{a.tag}", "--jq", ".sha").strip()
    if not (a.version and a.sums):
        sys.exit("render_manifests: give --tag, or --version and --sums")

    sums = read_sums(a.sums)
    subs = {
        "VERSION": a.version,
        "MSI_VERSION": a.msi_version or msi_version(a.version),
        "PKGVER": a.version.replace("-", "_"),  # an Arch pkgver cannot hold a hyphen
        "REPO": a.repo,
        "COMMIT": a.commit,
        "RELEASE_DATE": release_date,
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
        code = ZERO_GUID
        env = Path(a.state or "") / f"windows-{arch}.env"
        if a.state and env.is_file():
            for line in env.read_text().splitlines():
                if line.startswith("PRODUCT_CODE="):
                    code = line.split("=", 1)[1].strip()
        if code.strip("{}0-") == "":  # absent, or an all-zero placeholder: never a real code
            code = ZERO_GUID
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
