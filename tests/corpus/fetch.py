#!/usr/bin/env python3
"""Download the Panoply test corpus into tests/corpus/cache/ and verify every hash.

    python3 -I tests/corpus/fetch.py                 download whatever is missing, verify all
    python3 -I tests/corpus/fetch.py --kind pdf,image
    python3 -I tests/corpus/fetch.py --id pdf-qpdf-digitally-signed
    python3 -I tests/corpus/fetch.py --check         verify what is on disk, download nothing
    python3 -I tests/corpus/fetch.py --list          show the plan and the license of each file
    python3 -I tests/corpus/fetch.py --permissive-only   skip share-alike and copyleft licenses
    python3 -I tests/corpus/fetch.py --fill          pin sha256 and size for entries that lack them

Files already present with the right size and sha256 are skipped. A file that fails its
hash is deleted and reported, never kept. Entries with source = "generated" come from
generate.py instead. Standard library only. Run with -I so nothing in the working
directory can shadow a module.
"""
from __future__ import annotations

import argparse
import importlib.util
import os
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor

_here = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("_corpus", os.path.join(_here, "_corpus.py"))
C = importlib.util.module_from_spec(_spec)
sys.modules["_corpus"] = C
_spec.loader.exec_module(C)

UA = "panoply-corpus-fetch/0.1 (+https://github.com/anzal1/panoply; test corpus downloader)"
# Hosts that rate limit scripts. One connection at a time with a gap between requests.
POLITE_HOSTS = ("wikimedia.org", "archive.org", "pixls.us", "test-videos.co.uk", "irs.gov", "nist.gov", "govinfo.gov")
POLITE_GAP_S = 1.0

_host_locks: dict[str, threading.Lock] = {}
_host_last: dict[str, float] = {}
_registry_lock = threading.Lock()


def host_of(url: str) -> str:
    return url.split("/")[2] if "://" in url else ""


def polite_wait(host: str):
    if not any(host.endswith(h) for h in POLITE_HOSTS):
        return None
    with _registry_lock:
        lock = _host_locks.setdefault(host, threading.Lock())
    lock.acquire()
    gap = POLITE_GAP_S - (time.time() - _host_last.get(host, 0))
    if gap > 0:
        time.sleep(gap)
    return lock


def polite_done(host: str, lock):
    if lock is not None:
        _host_last[host] = time.time()
        lock.release()


def download(url: str, dest: str, retries: int = 4) -> None:
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    part = dest + ".part"
    host = host_of(url)
    last: Exception | None = None
    for attempt in range(retries):
        lock = polite_wait(host)
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=60) as r, open(part, "wb") as out:
                while True:
                    chunk = r.read(1 << 20)
                    if not chunk:
                        break
                    out.write(chunk)
            os.replace(part, dest)
            return
        except urllib.error.HTTPError as e:
            last = e
            if e.code in (404, 403, 410):
                break
            time.sleep(2 * (attempt + 1) if e.code != 429 else 15 * (attempt + 1))
        except Exception as e:  # network hiccup
            last = e
            time.sleep(2 * (attempt + 1))
        finally:
            polite_done(host, lock)
            if os.path.exists(part) and not os.path.exists(dest):
                try:
                    os.remove(part)
                except OSError:
                    pass
    raise RuntimeError(f"download failed: {last}")


def process(entry: dict, args) -> tuple[str, str, int]:
    """Returns (status, message, bytes_downloaded). Status: ok, cached, filled, fail, skip."""
    path = C.cache_path(entry["kind"], entry)
    pinned = entry["sha256"]
    if entry.get("source") == "generated":
        return "skip", "generated (run generate.py)", 0
    if os.path.exists(path):
        size_ok = entry["size"] == 0 or os.path.getsize(path) == entry["size"]
        if size_ok:
            digest = C.sha256_file(path)
            if not pinned and args.fill:
                C.fill_hash(entry["kind"], entry["id"], digest, os.path.getsize(path))
                return "filled", digest[:12], 0
            if digest == pinned:
                if not entry["size"] and args.fill:
                    C.fill_hash(entry["kind"], entry["id"], digest, os.path.getsize(path))
                return "cached", "", 0
        if args.check:
            return "fail", "hash or size mismatch on disk", 0
        os.remove(path)
    if args.check:
        return "fail", "missing", 0
    if not pinned and not args.fill:
        return "fail", "no sha256 pinned (run with --fill once, then review the diff)", 0
    try:
        download(entry["url"], path)
    except RuntimeError as e:
        return "fail", str(e), 0
    n = os.path.getsize(path)
    digest = C.sha256_file(path)
    if not pinned:
        C.fill_hash(entry["kind"], entry["id"], digest, n)
        return "filled", digest[:12], n
    if digest != pinned:
        os.remove(path)
        return "fail", f"sha256 mismatch: got {digest[:16]}, pinned {pinned[:16]}", n
    if entry["size"] and n != entry["size"]:
        os.remove(path)
        return "fail", f"size mismatch: got {n}, pinned {entry['size']}", n
    if not entry["size"] and args.fill:
        C.fill_hash(entry["kind"], entry["id"], digest, n)
    return "ok", "", n


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--kind", help="comma list of: " + ", ".join(C.KINDS))
    ap.add_argument("--id", action="append", help="only this id (repeatable)")
    ap.add_argument("--tag", action="append", help="only entries with this tag (repeatable)")
    ap.add_argument("--check", action="store_true", help="verify only, never download")
    ap.add_argument("--list", action="store_true", help="print the plan and exit")
    ap.add_argument("--fill", action="store_true", help="pin sha256 and size where missing")
    ap.add_argument("--permissive-only", action="store_true", help="skip CC-BY-SA, LGPL and other copyleft")
    ap.add_argument("--max-total-mb", type=int, default=1500, help="refuse to plan more than this (default 1500)")
    ap.add_argument("--jobs", type=int, default=6)
    args = ap.parse_args()

    kinds = tuple(args.kind.split(",")) if args.kind else C.KINDS
    for k in kinds:
        if k not in C.KINDS:
            C.die(f"unknown kind {k!r}")
    entries = C.select(C.load_all(kinds), ids=args.id, tags=args.tag, permissive_only=args.permissive_only)
    remote = [e for e in entries if e.get("source") != "generated"]

    planned = sum(e["size"] for e in remote)
    if planned > args.max_total_mb * 1024 * 1024:
        C.die(f"plan is {C.human(planned)}, over the {args.max_total_mb} MB cap")

    if args.list:
        for e in entries:
            src = "generated" if e.get("source") == "generated" else host_of(e["url"])
            print(f"{e['kind']:7} {e['id']:46} {C.human(e['size']):>9}  {e['license']:16} {src}")
        print(f"\n{len(entries)} entries, {len(remote)} to download, {C.human(planned)} total")
        return 0

    t0 = time.time()
    results: list[tuple[dict, str, str, int]] = []
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futs = [(e, pool.submit(process, e, args)) for e in entries]
        for e, f in futs:
            status, msg, n = f.result()
            results.append((e, status, msg, n))
            if status in ("fail", "filled", "ok"):
                print(f"{status:7} {e['kind']:6} {e['id']}  {msg}", flush=True)

    counts: dict[str, int] = {}
    downloaded = 0
    for _, s, _, n in results:
        counts[s] = counts.get(s, 0) + 1
        downloaded += n
    on_disk = sum(
        os.path.getsize(C.cache_path(e["kind"], e)) for e, s, _, _ in results if s in ("ok", "cached", "filled")
    )
    print(
        f"\n{len(results)} entries: "
        + ", ".join(f"{v} {k}" for k, v in sorted(counts.items()))
        + f". Downloaded {C.human(downloaded)}, {C.human(on_disk)} verified on disk, {time.time() - t0:.0f}s."
    )
    return 1 if counts.get("fail") else 0


if __name__ == "__main__":
    sys.exit(main())
