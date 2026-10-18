#!/usr/bin/env python3
"""Print the one-click install links for `panoply mcp` and prove they decode to the right command.

    python3 -I tools/mcp_links.py            print the links
    python3 -I tools/mcp_links.py --check    decode every link and fail unless it runs `panoply mcp`

Formats (all three are the vendors' own, checked against their docs and the playwright-mcp README):
  Cursor   cursor://anysphere.cursor-deeplink/mcp/install?name=NAME&config=BASE64(JSON of one server)
           https://cursor.com/en/install-mcp?name=NAME&config=...   (the web form, works in a README)
  VS Code  vscode:mcp/install?URLENCODED(JSON {"name","command","args"})
           vscode-insiders:mcp/install?...   (Insiders)
           https://insiders.vscode.dev/redirect?url=URLENCODED(the vscode: link)   (GitHub strips vscode: links)
"""
import base64
import json
import sys
from urllib.parse import parse_qs, quote, unquote, urlsplit

NAME = "panoply"
SERVER = {"command": "panoply", "args": ["mcp"]}


def links():
    cfg = base64.b64encode(json.dumps(SERVER, separators=(",", ":")).encode()).decode()
    vs = quote(json.dumps({"name": NAME, **SERVER}, separators=(",", ":")), safe="")
    return {
        "cursor": f"cursor://anysphere.cursor-deeplink/mcp/install?name={NAME}&config={quote(cfg, safe='')}",
        "cursor_web": f"https://cursor.com/en/install-mcp?name={NAME}&config={quote(cfg, safe='')}",
        "vscode": f"vscode:mcp/install?{vs}",
        "vscode_insiders": f"vscode-insiders:mcp/install?{vs}",
        "vscode_web": "https://insiders.vscode.dev/redirect?url=" + quote(f"vscode:mcp/install?{vs}", safe=""),
        "vscode_insiders_web": "https://insiders.vscode.dev/redirect?url="
        + quote(f"vscode-insiders:mcp/install?{vs}", safe=""),
    }


def decode(kind, url):
    """The server entry a link installs, as the vendor's installer would read it."""
    if kind.endswith("_web") and kind.startswith("vscode"):
        url = unquote(parse_qs(urlsplit(url).query)["url"][0])
        return decode("vscode", url)
    if kind.startswith("cursor"):
        q = parse_qs(urlsplit(url).query)
        return {"name": q["name"][0], **json.loads(base64.b64decode(q["config"][0]))}
    return json.loads(unquote(url.split("mcp/install?", 1)[1]))


if __name__ == "__main__":
    ls = links()
    ok = True
    for k, u in ls.items():
        d = decode(k, u)
        good = d.get("name") == NAME and d["command"] == "panoply" and d["args"] == ["mcp"]
        ok &= good
        if "--check" in sys.argv:
            print(f"{'ok ' if good else 'BAD'} {k:20} -> {json.dumps(d, separators=(',', ':'))}")
        else:
            print(f"{k}\n{u}\n")
    sys.exit(0 if ok else 1)
