#!/usr/bin/env python3
"""Fill in server.json for the official MCP registry from the .mcpb bundles of a release.

    render_server_json.py --version 0.2.0 --tag v0.2.0 --repo anzal1/panoply --dir release --out server.json
    render_server_json.py --check server.json     (validate against the registry schema; needs jsonschema)

One `mcpb` package per bundle, each with the SHA-256 of the file in --dir. The registry wants the
download URL to hold "mcp" (it does: panoply-mcp-VERSION-ID.mcpb) and the host to be a GitHub release.
Run with `python3 -I`.
"""
import argparse
import hashlib
import json
import sys
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCHEMA_URL = "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json"
IDS = ["macos-arm64", "macos-x64", "windows-x64", "windows-arm64", "linux-x64", "linux-arm64"]


def render(a):
    doc = json.loads((HERE / "server.template.json").read_text())
    doc["version"] = a.version
    for i in IDS:
        name = f"panoply-mcp-{a.version}-{i}.mcpb"
        f = Path(a.dir) / name
        if not f.is_file():
            sys.exit(f"render_server_json: {f} is missing")
        doc["packages"].append(
            {
                "registryType": "mcpb",
                "identifier": f"https://github.com/{a.repo}/releases/download/{a.tag}/{name}",
                "fileSha256": hashlib.sha256(f.read_bytes()).hexdigest(),
                "transport": {"type": "stdio"},
            }
        )
    Path(a.out).write_text(json.dumps(doc, indent=2) + "\n")
    print(f"wrote {a.out}: {len(doc['packages'])} mcpb packages, version {a.version}")


def check(path):
    import jsonschema  # pip install jsonschema

    schema = json.load(urllib.request.urlopen(SCHEMA_URL))
    doc = json.load(open(path))
    jsonschema.validate(doc, schema)
    for p in doc["packages"]:
        assert p["registryType"] == "mcpb" and "mcp" in p["identifier"], p
        assert p["identifier"].startswith("https://github.com/"), p
    print(f"{path}: valid against {SCHEMA_URL}")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--check")
    ap.add_argument("--version")
    ap.add_argument("--tag")
    ap.add_argument("--repo")
    ap.add_argument("--dir")
    ap.add_argument("--out")
    a = ap.parse_args()
    if a.check:
        check(a.check)
    elif all([a.version, a.tag, a.repo, a.dir, a.out]):
        render(a)
    else:
        ap.error("give --check FILE, or --version --tag --repo --dir --out")
