#!/usr/bin/env python3
"""Decide which CI jobs a push or pull request needs, from the files it changed.

    python3 -I tools/ci/changes.py                 in CI: reads the event, asks the GitHub API for the files
    printf 'README.md\\n' | python3 -I tools/ci/changes.py --files     the same decision for a list on stdin

Prints one `name=true|false` line per output (and appends them to $GITHUB_OUTPUT when it is set):

    code         the Rust jobs: fmt, clippy and the tests on every OS
    deps         the dependency checks (licenses, advisories): code changed, or a manifest or deny.toml did
    notices      the THIRD-PARTY-NOTICES.md check
    conversions  the conversion graph check

The docs check is not listed because it always runs (it takes a second).

When in doubt it runs everything: a manual run, a schedule, a missing base commit, an API error or a
changed-file list that GitHub cut short all give `true` for every output. Documentation counts as
documentation only where no build or test can read it: Markdown outside src/ and tests/ folders,
everything under docs/ (pictures included, the app's own icons live elsewhere), licenses and the issue templates. See docs/CI.md.
"""
import fnmatch
import json
import os
import subprocess
import sys

OUTPUTS = ("code", "deps", "notices", "conversions")

# Files that never reach a build or a test.
DOC_GLOBS = (
    "docs/*",
    "LICENSE", "LICENSE-*", "COPYING*",
    ".github/ISSUE_TEMPLATE/*", ".github/PULL_REQUEST_TEMPLATE*", ".github/CODEOWNERS", ".github/FUNDING.yml",
    ".github/dependabot.yml",
    ".gitignore", ".gitattributes", ".editorconfig",
)
# Markdown is documentation unless a crate can pull it in at compile time or a test reads it.
MD_NOT_DOCS = ("/src/", "/tests/", "/patches/", "/fixtures/")

# Own jobs, not Rust jobs.
CONVERSION_FILES = ("docs/CONVERSIONS.md", "conversions/*")
NOTICE_FILES = ("THIRD-PARTY-NOTICES.md", "tools/notices/*", "Cargo.lock", "modules/examples-wasm/*/Cargo.lock")
DEP_FILES = ("Cargo.lock", "Cargo.toml", "*/Cargo.toml", "*/*/Cargo.toml", "deny.toml", ".github/workflows/*")


def match(path, globs):
    return any(fnmatch.fnmatch(path, g) for g in globs)


def is_docs(path):
    if match(path, CONVERSION_FILES) or path == "THIRD-PARTY-NOTICES.md":
        return True  # handled by their own jobs
    if path.endswith(".md"):
        return not any(part in "/" + path for part in MD_NOT_DOCS)
    return match(path, DOC_GLOBS)


def decide(files):
    code = any(not is_docs(f) for f in files)
    return {
        "code": code,
        "deps": code or any(match(f, DEP_FILES) for f in files),
        "notices": code or any(match(f, NOTICE_FILES) for f in files),
        "conversions": code or any(match(f, CONVERSION_FILES) for f in files),
    }


def api(path):
    out = subprocess.check_output(["gh", "api", "--paginate", "--slurp", path])
    pages = json.loads(out)
    return pages


def changed_files():
    """The changed paths for this event, or None when the answer is 'everything'."""
    event = os.environ.get("EVENT_NAME", "")
    repo = os.environ.get("REPO", "")
    try:
        if event == "pull_request":
            n = os.environ["PR_NUMBER"]
            pages = api(f"repos/{repo}/pulls/{n}/files?per_page=100")
            files = [f for page in pages for f in page]
            return None if len(files) >= 3000 else [f["filename"] for f in files] + [
                f.get("previous_filename", "") for f in files if f.get("previous_filename")
            ]
        if event == "push":
            before, after = os.environ["BEFORE"], os.environ["AFTER"]
            if set(before) <= {"0"}:
                return None
            pages = api(f"repos/{repo}/compare/{before}...{after}?per_page=100")
            files = [f for page in pages for f in page["files"]]
            total = pages[0].get("total_commits", 0)
            # The compare API lists at most 300 files; a longer list is cut off.
            if len(files) >= 300 or total == 0:
                return None
            return [f["filename"] for f in files] + [
                f.get("previous_filename", "") for f in files if f.get("previous_filename")
            ]
    except Exception as e:  # noqa: BLE001 - any failure means run everything
        print(f"could not list the changed files ({e}); running everything", file=sys.stderr)
    return None


def main():
    if "--files" in sys.argv:
        files = [l.strip() for l in sys.stdin if l.strip()]
    else:
        files = changed_files()
    if files is None:
        result = {k: True for k in OUTPUTS}
    else:
        result = decide([f for f in files if f])
        print(f"{len(files)} changed file(s)", file=sys.stderr)
    lines = [f"{k}={'true' if result[k] else 'false'}" for k in OUTPUTS]
    print("\n".join(lines))
    path = os.environ.get("GITHUB_OUTPUT")
    if path:
        with open(path, "a") as f:
            f.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
