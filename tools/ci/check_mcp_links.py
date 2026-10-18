#!/usr/bin/env python3
"""Fail when an install link in README.md or docs/MCP.md differs from what tools/mcp_links.py makes.

    python3 -I tools/ci/check_mcp_links.py

Every Cursor and VS Code install link found in those two files must be one of the generated links, and the
generated web links (the ones that work on GitHub) must all be present in docs/MCP.md and in README.md
(the Cursor and VS Code buttons). tools/mcp_links.py --check proves the links decode to `panoply mcp`; this proves
the Markdown still holds those exact links. Run by tools/preflight.sh and by the docs job in CI.
"""
import importlib.util
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("mcp_links", ROOT / "tools" / "mcp_links.py")
mcp_links = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mcp_links)

LINK = re.compile(
    r"(?:cursor://anysphere\.cursor-deeplink/mcp/install\?|https://cursor\.com/en/install-mcp\?"
    r"|vscode(?:-insiders)?:mcp/install\?|https://insiders\.vscode\.dev/redirect\?url=)[^\s)`\"']+"
)


def main():
    want = mcp_links.links()
    allowed = set(want.values())
    problems = []
    found = {}
    for name in ("README.md", "docs/MCP.md"):
        text = (ROOT / name).read_text()
        found[name] = set(LINK.findall(text))
        for link in sorted(found[name] - allowed):
            problems.append(f"{name}: link is not what tools/mcp_links.py makes: {link[:90]}")
    for key in ("cursor_web", "vscode_web", "vscode_insiders_web", "cursor", "vscode", "vscode_insiders"):
        if want[key] not in found["docs/MCP.md"]:
            problems.append(f"docs/MCP.md: missing the {key} link")
    for key in ("cursor_web", "vscode_web"):
        if want[key] not in found["README.md"]:
            problems.append(f"README.md: missing the {key} link")
    if problems:
        sys.exit("check_mcp_links:\n  " + "\n  ".join(problems) + "\nRun python3 tools/mcp_links.py and paste the links.")
    print(f"install links in README.md and docs/MCP.md match tools/mcp_links.py ({len(allowed)} generated)")


if __name__ == "__main__":
    main()
