#!/usr/bin/env python3
"""Check rendered manifests: the winget YAML against Microsoft's JSON schemas, the Flatpak manifest and the
metainfo for well-formedness, the Scoop manifest as JSON.

    validate_manifests.py DIR            DIR is the --out of render_manifests.py
    validate_manifests.py DIR --offline  reuse schemas cached in ~/.cache/panoply/winget-schemas

Needs PyYAML and jsonschema (pip install pyyaml jsonschema). appstreamcli validate and makepkg
--printsrcinfo are separate checks (tools/validate-packaging.sh runs them in a container).
"""
import json
import re
import sys
import urllib.request
import xml.dom.minidom
from pathlib import Path

import jsonschema
import yaml

SCHEMA_URL = "https://raw.githubusercontent.com/microsoft/winget-cli/master/schemas/JSON/manifests/v{v}/manifest.{kind}.{v}.json"
CACHE = Path.home() / ".cache/panoply/winget-schemas"
KINDS = {"version": "version", "installer": "installer", "defaultLocale": "defaultLocale"}


def schema(kind, version, offline):
    path = CACHE / f"{kind}.{version}.json"
    if not path.is_file():
        if offline:
            sys.exit(f"no cached schema {path}")
        CACHE.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(SCHEMA_URL.format(v=version, kind=kind), timeout=30) as r:
            path.write_bytes(r.read())
    return json.loads(path.read_text())


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    offline = "--offline" in sys.argv
    if len(args) != 1:
        sys.exit(__doc__)
    out = Path(args[0])
    failed = 0
    for f in sorted((out / "winget").glob("*/*.yaml")):
        doc = yaml.safe_load(f.read_text())
        doc = json.loads(json.dumps(doc, default=str))  # dates become strings, as the schema expects
        kind, version = doc["ManifestType"], doc["ManifestVersion"]
        errs = sorted(jsonschema.Draft7Validator(schema(kind, version, offline)).iter_errors(doc), key=str)
        if not re.search(rf"manifest\.{kind}\.{re.escape(version)}\.schema\.json", f.read_text().splitlines()[0]):
            errs.append(f"modeline schema does not match {kind} {version}")
        print(f"{'FAIL' if errs else 'ok  '} {f.relative_to(out)}  (schema {kind} {version})")
        for e in errs:
            failed += 1
            print("     ", getattr(e, "message", e), list(getattr(e, "path", [])))
    for f in sorted((out / "flatpak").glob("*.yml")):
        yaml.safe_load(f.read_text()); print(f"ok   {f.relative_to(out)}  (YAML parses)")
    for f in sorted((out / "flatpak").glob("*.xml")):
        xml.dom.minidom.parseString(f.read_bytes()); print(f"ok   {f.relative_to(out)}  (XML parses)")
    json.loads((out / "scoop/panoply.json").read_text()); print("ok   scoop/panoply.json  (JSON parses)")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
