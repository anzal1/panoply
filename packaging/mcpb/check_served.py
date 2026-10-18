#!/usr/bin/env python3
"""Fail unless `panoply mcp`, run from inside a built bundle, serves exactly the tools its manifest lists.

    check_served.py BUNDLE.mcpb      (the bundle must be for the system this runs on)

Unpacks the bundle, starts `server/panoply mcp` with an empty home folder (no user modules, no source tree) the
way a release install sees it, asks for tools/list and compares names and descriptions with manifest.json.
Prints the count, so the number in the docs and the manifest is the number the server really serves.
Run with `python3 -I`.
"""
import json
import os
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path


def main():
    bundle = Path(sys.argv[1])
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "b"
        with zipfile.ZipFile(bundle) as z:
            z.extractall(root)
            for i in z.infolist():
                mode = i.external_attr >> 16
                if mode:
                    os.chmod(root / i.filename, mode)
        manifest = json.loads((root / "manifest.json").read_text())
        exe = root / manifest["server"]["entry_point"]
        home = Path(tmp) / "home"
        home.mkdir()
        msgs = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "check", "version": "1"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        ]
        env = {**os.environ, "HOME": str(home), "XDG_DATA_HOME": str(home), "XDG_CONFIG_HOME": str(home),
               "LOCALAPPDATA": str(home), "APPDATA": str(home), "PANOPLY_NO_SOURCE_TREE": "1"}
        env.pop("PANOPLY_MODULES", None)
        r = subprocess.run([str(exe), "mcp"], input="\n".join(map(json.dumps, msgs)) + "\n", text=True,
                           capture_output=True, env=env, cwd=tmp, timeout=60)
        served = {}
        for line in r.stdout.splitlines():
            d = json.loads(line)
            if d.get("id") == 2:
                served = {t["name"]: t["description"] for t in d["result"]["tools"]}
        listed = {t["name"]: t["description"] for t in manifest["tools"]}
        problems = []
        for n in sorted(served.keys() - listed.keys()):
            problems.append(f"served but not in the manifest: {n}")
        for n in sorted(listed.keys() - served.keys()):
            problems.append(f"in the manifest but not served: {n}")
        for n in sorted(served.keys() & listed.keys()):
            if served[n] != listed[n]:
                problems.append(f"description differs for {n}")
        print(f"served {len(served)} tools, manifest lists {len(listed)} ({bundle.name})")
        if not served or problems:
            sys.exit("check_served: " + "; ".join(problems or ["no tools served: " + r.stderr[-300:]]))
        print("manifest matches what `panoply mcp` serves")


if __name__ == "__main__":
    main()
