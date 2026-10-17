#!/usr/bin/env python3
"""Prose lint: no em dashes and no stock AI phrasing in what users read.

    python3 -I tools/ci/prose_lint.py                  files only
    python3 -I tools/ci/prose_lint.py --commits RANGE  files, and the messages of the commits in RANGE
                                                       (for example origin/main..HEAD)
    python3 -I tools/ci/prose_lint.py PATH...          only these files (used for tests and by hand)

What is read:
  * every tracked Markdown file (README.md, CHANGELOG.md, docs/**, ...), outside code blocks, inline code and
    HTML comments. A phrase wrapped over two lines is found (the text is read a paragraph at a time).
  * packaging metainfo and manifests (packaging/ and apps/desktop/packaging/, text formats only)
  * apps/desktop/src: the contents of string literals only. Comments and code are not read.
  * module.toml: the title and description values
  * commit messages, when --commits is given
Left out: THIRD-PARTY-NOTICES.md and tools/notices (third-party text), apps/desktop/patches/ (vendored
upstream files), data files such as OCR dictionaries.

Fails on the em dash (U+2014, also when written as the escape \\u{2014} in a Rust string), except inside a
URL, and on the phrases in tools/ci/prose-lint-phrases.txt. One line per hit: file:line: "word" suggestion.
Exit status 1 on any hit.
"""
import os
import re
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
PHRASES_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "prose-lint-phrases.txt")

EM = "\u2014"
EM_SUGGESTION = "rewrite the sentence plainly (a full stop, a comma, a colon or brackets); do not swap in a hyphen"

SKIP_TREES = ("apps/desktop/patches/", "tools/notices/")
SKIP_FILES = ("THIRD-PARTY-NOTICES.md",)
MANIFEST_TREES = ("packaging/", "apps/desktop/packaging/")
MANIFEST_EXTS = (".in", ".xml", ".yaml", ".yml", ".json", ".rb", ".desktop", ".wxs", ".plist", ".md")

URL = re.compile(r"(?:https?|ftp)://[^\s<>\"')\]]+|mailto:[^\s<>\"')\]]+")
FENCE = re.compile(r"^\s*(```+|~~~+)")
INLINE_CODE = re.compile(r"(`+)(?:(?!\1).)+\1", re.S)
HTML_COMMENT = re.compile(r"<!--.*?-->", re.S)


def load_phrases():
    out = []
    with open(PHRASES_FILE, encoding="utf-8") as f:
        for raw in f:
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            phrase, _, hint = line.partition("|")
            phrase = phrase.strip().lower()
            if phrase:
                rx = re.compile(r"(?<![\w-])" + re.escape(phrase).replace(r"\ ", r"\s+") + r"(?![\w-])", re.I)
                out.append((phrase, hint.strip() or "say it plainly", rx))
    return out


PHRASES = load_phrases()


def scan_text(text):
    """Yield (offset_in_text, word, suggestion) for one piece of prose. URLs are blanked out first."""
    # a typographic apostrophe is the same word as a plain one (same length, so offsets stay valid)
    blanked = URL.sub(lambda m: " " * len(m.group(0)), text.replace("\u2019", "'"))
    for m in re.finditer(EM, blanked):
        yield m.start(), "em dash", EM_SUGGESTION
    for phrase, hint, rx in PHRASES:
        for m in rx.finditer(blanked):
            yield m.start(), phrase, hint


def blank(m):
    """The same text with every character but the newlines turned into a space (offsets and lines stay put)."""
    return re.sub(r"[^\n]", " ", m.group(0))


def scan_paragraphs(lines):
    """Yield (line_number, word, suggestion) per paragraph, so a phrase wrapped over two lines is found."""
    para, first = [], 0
    for n, line in enumerate(lines + [""], 1):
        if line.strip():
            if not para:
                first = n
            para.append(line)
            continue
        if para:
            text = "\n".join(para)
            for off, word, hint in scan_text(text):
                yield first + text.count("\n", 0, off), word, hint
            para = []


def lint_prose_lines(rel, lines, hits, skip_code=False):
    if skip_code:
        # fenced code blocks become empty lines (paragraph breaks), then HTML comments are blanked
        kept, in_fence = [], None
        for line in lines:
            fm = FENCE.match(line)
            if fm:
                marker = fm.group(1)[0]
                if in_fence is None:
                    in_fence = marker
                elif in_fence == marker:
                    in_fence = None
                kept.append("")
            else:
                kept.append("" if in_fence else line)
        text = HTML_COMMENT.sub(blank, "\n".join(kept))
        lines = text.split("\n")
        # inline code, inside one paragraph at a time
        out, para = [], []
        for line in lines + [""]:
            if line.strip():
                para.append(line)
            else:
                out.extend(INLINE_CODE.sub(blank, "\n".join(para)).split("\n") if para else [])
                out.append(line)
                para = []
        lines = out[:-1]
    for n, word, hint in scan_paragraphs(lines):
        hits.append((rel, n, word, hint))


# ---- Rust string literals ---------------------------------------------------------------------------
def rust_strings(src):
    """Yield (line, text, is_raw) for every string literal in Rust source. Comments, chars and lifetimes are skipped."""
    i, n, line = 0, len(src), 1
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
        elif src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
        elif src.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if src.startswith("/*", i):
                    depth += 1
                    i += 2
                elif src.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    line += src[i] == "\n"
                    i += 1
        elif c in "rbc" and re.match(r"(?:br|cr|r)#*\"", src[i:i + 12]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.match(r"(?:br|cr|r)(#*)\"", src[i:i + 12])
            hashes = m.group(1)
            start = i + m.end()
            end = src.find('"' + hashes, start)
            if end < 0:
                return
            body = src[start:end]
            yield line, body, True
            line += body.count("\n")
            i = end + 1 + len(hashes)
        elif c == '"':
            j, buf, start_line = i + 1, [], line
            while j < n and src[j] != '"':
                if src[j] == "\\" and j + 1 < n:
                    buf.append(src[j:j + 2])
                    line += src[j + 1] == "\n"
                    j += 2
                else:
                    line += src[j] == "\n"
                    buf.append(src[j])
                    j += 1
            yield start_line, "".join(buf), False
            i = j + 1
        elif c == "'":
            # a char literal ('a', '\n', '"', '\u{2014}') or a lifetime ('a). Skip a char literal whole.
            m = re.match(r"'(?:\\u\{[0-9a-fA-F_]+\}|\\x[0-9a-fA-F]{2}|\\.|[^\\'])'", src[i:i + 14])
            i += m.end() if m else 1
        else:
            i += 1


def lint_rust(rel, text, hits):
    for line, lit, raw in rust_strings(text):
        if not raw:
            # One pass, so an escaped backslash followed by u{2014} stays text: "\\u{2014}" shows a backslash.
            # The escape \u{2014} is an em dash on screen.
            lit = re.sub(r"\\\\|\\u\{0*2014\}", lambda m: "\\" if m.group(0) == "\\\\" else EM, lit)
        for off, word, hint in scan_text(lit):
            hits.append((rel, line + lit.count("\n", 0, off), word, hint))


# ---- module.toml ------------------------------------------------------------------------------------
TOML_KEY = re.compile(r"""^\s*(title|description)\s*=\s*(\"\"\"|'''|\"|')""")


def lint_toml(rel, text, hits):
    lines = text.split("\n")
    n = 0
    while n < len(lines):
        m = TOML_KEY.match(lines[n])
        if not m:
            n += 1
            continue
        quote = m.group(2)
        rest = lines[n][m.end():]
        first = n + 1
        body = [rest]
        if len(quote) == 3:
            while quote not in body[-1] and n + 1 < len(lines):
                n += 1
                body.append(lines[n])
        value = "\n".join(body)
        for off, word, hint in scan_text(value):
            hits.append((rel, first + value.count("\n", 0, off), word, hint))
        n += 1


# ---- what to read -----------------------------------------------------------------------------------
def tracked_files():
    out = subprocess.check_output(["git", "-C", ROOT, "ls-files", "-z"], text=True)
    return [f for f in out.split("\0") if f]


def lint_file(rel, hits):
    if rel in SKIP_FILES or rel.startswith(SKIP_TREES):
        return
    path = os.path.join(ROOT, rel)
    if not os.path.isfile(path):
        return
    base = os.path.basename(rel)
    try:
        with open(path, encoding="utf-8") as f:
            text = f.read()
    except (UnicodeDecodeError, OSError):
        return
    if rel.endswith(".md"):
        lint_prose_lines(rel, text.split("\n"), hits, skip_code=True)
    elif rel.startswith("apps/desktop/src/") and rel.endswith(".rs"):
        lint_rust(rel, text, hits)
    elif base == "module.toml":
        lint_toml(rel, text, hits)
    elif rel.startswith(MANIFEST_TREES) and rel.endswith(MANIFEST_EXTS):
        lint_prose_lines(rel, text.split("\n"), hits)


class RangeError(Exception):
    pass


def lint_commits(rng, hits):
    run = subprocess.run(["git", "-C", ROOT, "log", "--no-merges", "--format=%H%x1f%B%x1e", rng],
                         capture_output=True, text=True)
    if run.returncode != 0:
        raise RangeError(f"range {rng} not found, run git fetch")
    for rec in run.stdout.split("\x1e"):
        rec = rec.strip("\n")
        if not rec:
            continue
        sha, _, msg = rec.partition("\x1f")
        for n, word, hint in scan_paragraphs(msg.split("\n")):
            hits.append((f"commit {sha[:10]}", n, word, hint))


def main(argv):
    commits = None
    paths = []
    it = iter(argv)
    for a in it:
        if a == "--commits":
            commits = next(it, None)
            if not commits:
                print("--commits needs a range such as origin/main..HEAD", file=sys.stderr)
                return 2
        elif a in ("-h", "--help"):
            print(__doc__)
            return 0
        else:
            paths.append(a)
    hits = []
    if paths:
        for p in paths:
            lint_file(os.path.relpath(os.path.abspath(p), ROOT), hits)
    else:
        for rel in tracked_files():
            lint_file(rel, hits)
    if commits:
        try:
            lint_commits(commits, hits)
        except RangeError as e:
            print(f"prose lint: {e}", file=sys.stderr)
            return 2
    for rel, n, word, hint in hits:
        print(f'{rel}:{n}: "{word}": {hint}')
    if hits:
        print(f"prose lint: {len(hits)} hit(s). See CONTRIBUTING.md, Writing.", file=sys.stderr)
        return 1
    print("prose lint: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
