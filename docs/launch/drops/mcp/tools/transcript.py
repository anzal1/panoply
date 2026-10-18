#!/usr/bin/env python3
"""Run a real `panoply mcp` session and record every JSON-RPC line with its time.

usage: transcript.py <panoply binary> <work dir> <out dir> [variant]

Makes two PDFs of 2 and 3 pages from generated photo-like pages, then speaks the protocol
over stdio exactly like a client: initialize, tools/list, pdf_merge, pdf_compress (under 1 MB).
Writes transcript.jsonl (one line per message, with ms since start), summary.json and tools-list.json.
"""
import json, os, subprocess, sys, threading, queue, time, random, hashlib
from pathlib import Path
from PIL import Image

binary, work, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), Path(sys.argv[3]).resolve()
variant = sys.argv[4] if len(sys.argv) > 4 else "a"
work.mkdir(parents=True, exist_ok=True); out.mkdir(parents=True, exist_ok=True)
amp = {"a": 60, "b": 28, "c": 12}[variant]

def page(seed):
    r = random.Random(seed)
    w, h = 1240, 1754
    small = Image.new("RGB", (16, 22))
    small.putdata([(r.randint(40, 230), r.randint(40, 230), r.randint(40, 230)) for _ in range(16 * 22)])
    base = small.resize((w, h), Image.BICUBIC)
    noise = Image.effect_noise((w, h), amp).convert("RGB")
    return Image.blend(base, noise, 0.25)

def make_pdf(path, pages, seed0):
    imgs = [page(seed0 + i) for i in range(pages)]
    imgs[0].save(path, "PDF", save_all=True, append_images=imgs[1:], resolution=150, quality=90)

a, b = work / "part-1.pdf", work / "part-2.pdf"
make_pdf(a, 2, 100); make_pdf(b, 3, 200)
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
before = {str(a): sha(a), str(b): sha(b)}
sizes_in = {a.name: a.stat().st_size, b.name: b.stat().st_size}

outdir = work / "out"
env = dict(os.environ, PANOPLY_DATA_DIR=str(work / "data"), PANOPLY_NO_SOURCE_TREE="1")
env.pop("PANOPLY_MODULES", None)
t0 = time.monotonic()
p = subprocess.Popen([str(binary), "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True, bufsize=1)
q = queue.Queue()
log = []
def reader():
    for line in p.stdout:
        q.put((time.monotonic(), line))
threading.Thread(target=reader, daemon=True).start()
def ms(t): return round((t - t0) * 1000, 1)
def send(msg):
    log.append({"t_ms": ms(time.monotonic()), "dir": "client->server", "msg": msg})
    p.stdin.write(json.dumps(msg) + "\n"); p.stdin.flush()
nid = [0]
def request(method, params):
    nid[0] += 1
    i = nid[0]
    send({"jsonrpc": "2.0", "id": i, "method": method, "params": params})
    while True:
        t, line = q.get(timeout=300)
        m = json.loads(line)
        log.append({"t_ms": ms(t), "dir": "server->client", "msg": m})
        if m.get("id") == i:
            return m
def call(name, args):
    return request("tools/call", {"name": name, "arguments": args, "_meta": {"progressToken": "job"}})

init = request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "panoply-drop", "version": "0"}})
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
tl_t0 = time.monotonic()
tl = request("tools/list", {})
tl_ms = round((time.monotonic() - tl_t0) * 1000, 1)
tools = tl["result"]["tools"]
(out / "tools-list.json").write_text(json.dumps(tools, indent=1))

m_t0 = time.monotonic()
merge = call("pdf_merge", {"files": [str(a), str(b)], "output_dir": str(outdir), "order": "given"})
merge_ms = round((time.monotonic() - m_t0) * 1000, 1)
merged = Path(merge["result"]["structuredContent"]["outputs"][0])
c_t0 = time.monotonic()
comp = call("pdf_compress", {"files": [str(merged)], "under_mb": 1, "output_dir": str(outdir)})
comp_ms = round((time.monotonic() - c_t0) * 1000, 1)
outs = comp["result"].get("structuredContent", {}).get("outputs", [])
compressed = Path(outs[0]) if outs else None
p.stdin.close()
try: p.wait(timeout=10)
except Exception: p.kill()
stderr = p.stderr.read()

pages = lambda path: len(__import__("re").findall(rb"/Type\s*/Page[^s]", path.read_bytes()))
summary = {
    "variant": variant,
    "tool_count": len(tools),
    "tool_names": [t["name"] for t in tools],
    "tools_list_ms": tl_ms,
    "inputs_bytes": sizes_in,
    "inputs_unchanged": before == {str(a): sha(a), str(b): sha(b)},
    "merge": {"ms": merge_ms, "isError": merge["result"].get("isError"), "outputs": merge["result"]["structuredContent"]["outputs"],
              "bytes": merged.stat().st_size, "pages_regex": pages(merged), "text": merge["result"]["content"][0]["text"]},
    "compress": {"ms": comp_ms, "isError": comp["result"].get("isError"), "outputs": outs,
                 "bytes": compressed.stat().st_size if compressed else None,
                 "pages_regex": pages(compressed) if compressed else None,
                 "text": comp["result"]["content"][0]["text"]},
    "server_stderr_tail": stderr[-600:],
}
(out / "summary.json").write_text(json.dumps(summary, indent=1))
(out / "transcript.jsonl").write_text("\n".join(json.dumps(e) for e in log) + "\n")
(out / "ls.txt").write_text(subprocess.run(["ls", "-l", str(work), str(outdir)], capture_output=True, text=True).stdout)
print(json.dumps({k: v for k, v in summary.items() if k != "tool_names"}, indent=1))
