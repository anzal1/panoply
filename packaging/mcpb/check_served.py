#!/usr/bin/env python3
"""Fail unless the bundle's own server, started the way Claude Desktop starts it, serves the tools its manifest lists.

    check_served.py BUNDLE.mcpb      (the bundle must be for the system this runs on; needs node on PATH)

Unpacks the bundle with every file at mode 0600 (what Claude Desktop does, mcpb issues 294 and 282), so the launcher's
chmod is part of what is tested. Runs the manifest's own command and arguments (node server/index.js) with an empty
home folder (no user modules, no source tree), asks for tools/list and compares names and descriptions with
manifest.json. Prints the count, so the number in the manifest is the number the server really serves.
Run with `python3 -I`.
"""
import json
import os
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path


def served_tools(stdout):
    """{name: description} from the tools/list answer; lines that are not JSON are reported, not fatal."""
    served, bad = {}, []
    for line in stdout.splitlines():
        if not line.strip():
            continue
        try:
            d = json.loads(line)
            if d.get("id") == 2:
                served = {t["name"]: t["description"] for t in d["result"]["tools"]}
        except (ValueError, KeyError, TypeError) as e:
            bad.append(f"unreadable line {line[:80]!r} ({e})")
    return served, bad


def main():
    bundle = Path(sys.argv[1])
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "b"
        with zipfile.ZipFile(bundle) as z:
            z.extractall(root)
            for i in z.infolist():
                if not i.is_dir():
                    os.chmod(root / i.filename, 0o600)
        manifest = json.loads((root / "manifest.json").read_text())
        cfg = manifest["server"]["mcp_config"]
        defaults = {k: str(v.get("default", "")).lower() for k, v in manifest.get("user_config", {}).items()}

        def fill(s):
            s = s.replace("${__dirname}", str(root))
            for k, v in defaults.items():
                s = s.replace("${user_config.%s}" % k, v)
            return s

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
        env.update({k: fill(v) for k, v in cfg.get("env", {}).items()})
        cmd = [fill(cfg["command"]), *[fill(a) for a in cfg["args"]]]
        r = subprocess.run(cmd, input="\n".join(map(json.dumps, msgs)) + "\n", text=True,
                           capture_output=True, env=env, cwd=tmp, timeout=60)
        served, bad = served_tools(r.stdout)
        listed = {t["name"]: t["description"] for t in manifest["tools"]}
        problems = list(bad)
        for n in sorted(served.keys() - listed.keys()):
            problems.append(f"served but not in the manifest: {n}")
        for n in sorted(listed.keys() - served.keys()):
            problems.append(f"in the manifest but not served: {n}")
        for n in sorted(served.keys() & listed.keys()):
            if served[n] != listed[n]:
                problems.append(f"description differs for {n}")
        print(f"served {len(served)} tools, manifest lists {len(listed)} ({bundle.name})")
        if not served or problems:
            sys.exit(
                "check_served: "
                + "; ".join(problems or ["no tools served"])
                + f"\nexit code {r.returncode}, stderr:\n{r.stderr[-1500:]}"
            )
        print("manifest matches what `panoply mcp` serves")


if __name__ == "__main__":
    main()
