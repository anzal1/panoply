#!/usr/bin/env python3
"""Build one MCP Bundle (.mcpb) for Claude Desktop: the panoply program and its modules folder.

    build_mcpb.py --version 0.2.0 --platform darwin|win32|linux --id macos-arm64 \
                  --stage STAGE --out DIR [--mcpb "npx -y @anthropic-ai/mcpb@2.1.2"]

STAGE holds `panoply` (or `panoply.exe`) and, optionally, `modules/`. The bundle layout is
    manifest.json  icon.png  LICENSE-*  THIRD-PARTY-NOTICES.md  server/panoply[.exe]  server/modules/
The program finds `modules/` next to itself, so nothing else is needed. The output is
`panoply-mcp-VERSION-ID.mcpb` ("mcp" is in the name because the official MCP registry requires it in the
download URL). The manifest's tool list is read from the module manifests so it cannot go stale.
The bundle is validated with the official mcpb tool before it is written. Run with `python3 -I`.
"""
import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def module_tools():
    """(tool name, summary) for every built-in and shipped community module, sorted by name."""
    folders = sorted((ROOT / "modules" / "builtin").iterdir())
    shipped = [
        l.split()[0]
        for l in (ROOT / "modules" / "shipped.txt").read_text().splitlines()
        if l.strip() and not l.startswith("#")
    ]
    folders += [ROOT / "modules" / "examples-wasm" / s for s in shipped]
    tools = {}
    for f in folders:
        text = (f / "module.toml").read_text()
        sec = re.search(r"^\[module\](.*?)(?=^\[|\Z)", text, re.S | re.M)
        mid = re.search(r'^id\s*=\s*"([^"]+)"', sec.group(1), re.M)
        summary = re.search(r'^summary\s*=\s*"((?:[^"\\]|\\.)*)"', sec.group(1), re.M)
        if not mid or not summary:
            sys.exit(f"build_mcpb: {f}/module.toml has no [module] id and summary")
        tools[mid.group(1).replace(".", "_")] = json.loads(f'"{summary.group(1)}"')
    return [{"name": n, "description": tools[n]} for n in sorted(tools)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", required=True)
    ap.add_argument("--platform", required=True, choices=["darwin", "win32", "linux"])
    ap.add_argument("--id", required=True, help="macos-arm64, windows-x64, linux-x64 ...")
    ap.add_argument("--stage", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--mcpb", default="npx -y @anthropic-ai/mcpb@2.1.2")
    a = ap.parse_args()

    exe = "panoply.exe" if a.platform == "win32" else "panoply"
    stage = Path(a.stage)
    if not (stage / exe).is_file():
        sys.exit(f"build_mcpb: {stage / exe} is missing")
    manifest = (HERE / "manifest.template.json").read_text()
    for k, v in {"@VERSION@": a.version, "@EXE@": exe, "@PLATFORM@": a.platform}.items():
        manifest = manifest.replace(k, v)
    m = json.loads(manifest)
    m["tools"] = module_tools()

    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    target = out / f"panoply-mcp-{a.version}-{a.id}.mcpb"
    with tempfile.TemporaryDirectory() as tmp:
        b = Path(tmp) / "bundle"
        (b / "server").mkdir(parents=True)
        shutil.copy2(stage / exe, b / "server" / exe)
        (b / "server" / exe).chmod(0o755)
        if (stage / "modules").is_dir():
            shutil.copytree(stage / "modules", b / "server" / "modules")
        shutil.copy2(ROOT / "packaging" / "assets" / "panoply-512.png", b / "icon.png")
        for f in ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md"):
            if (ROOT / f).is_file():
                shutil.copy2(ROOT / f, b / f)
        (b / "manifest.json").write_text(json.dumps(m, indent=2) + "\n")
        mcpb = a.mcpb.split()
        subprocess.run([*mcpb, "validate", str(b / "manifest.json")], check=True)
        subprocess.run([*mcpb, "pack", str(b), str(target)], check=True)
    print(f"wrote {target} ({target.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
