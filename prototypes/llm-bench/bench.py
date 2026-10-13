#!/usr/bin/env python3
"""Benchmark small local LLMs for Panoply's document tasks through Ollama.

usage: python3 bench.py <ollama-model-tag> [more tags...]
Writes results/<tag>.json and prints a summary line per model.
"""
import json, os, re, subprocess, sys, threading, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
API = "http://127.0.0.1:11434"
CTX = 12288
read = lambda f: open(os.path.join(HERE, f)).read()
DOC, TRANSCRIPT, DICTATION, HINDI = read("doc.txt"), read("transcript.txt"), read("dictation.txt"), read("hindi.txt")
QA, GOLD = json.loads(read("qa.json")), json.loads(read("gold.json"))


def post(path, body, stream=False):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    return urllib.request.urlopen(req, timeout=600)


class PeakMem(threading.Thread):
    """Samples resident memory of Ollama's model runner processes."""
    def __init__(self):
        super().__init__(daemon=True); self.peak = 0; self.stop = False
    def run(self):
        while not self.stop:
            out = subprocess.run(["ps", "-axo", "rss=,command="], capture_output=True, text=True).stdout
            total = sum(int(l.split(None, 1)[0]) for l in out.splitlines()
                        if "ollama" in l and ("runner" in l or "llama" in l) and "serve" not in l)
            self.peak = max(self.peak, total); time.sleep(0.1)


def gen(model, prompt, system=None, predict=400, fmt=None):
    body = {"model": model, "prompt": prompt, "stream": True,
            "options": {"temperature": 0, "num_ctx": CTX, "num_predict": predict}}
    if system: body["system"] = system
    if fmt: body["format"] = fmt
    t0 = time.time(); ttft = None; text = []; final = {}
    for line in post("/api/generate", body):
        d = json.loads(line)
        if d.get("response"):
            if ttft is None: ttft = time.time() - t0
            text.append(d["response"])
        if d.get("done"): final = d
    out = "".join(text)
    out = re.sub(r"<think>.*?</think>", "", out, flags=re.S).strip()
    return out, {
        "ttft_s": round(ttft or 0, 3),
        "prompt_tok": final.get("prompt_eval_count", 0),
        "prompt_tok_s": round(final.get("prompt_eval_count", 0) / max(final.get("prompt_eval_duration", 1), 1) * 1e9, 1),
        "gen_tok": final.get("eval_count", 0),
        "gen_tok_s": round(final.get("eval_count", 0) / max(final.get("eval_duration", 1), 1) * 1e9, 1),
        "load_s": round(final.get("load_duration", 0) / 1e9, 2),
    }


def has(text, needles):
    t = text.lower().replace(",", "")
    return all(n.lower().replace(",", "") in t for n in needles)


def run(model):
    post("/api/generate", {"model": model, "keep_alive": 0}).read()  # unload first, measure cold load
    mem = PeakMem(); mem.start()
    res = {"model": model, "tasks": {}}

    out, st = gen(model, f"{DOC}\n\nSummarize the rules in this handbook that include a number, as up to 10 short bullet points. Keep every number exact.", predict=500)
    keys = [["45"], ["32"], ["21"], ["3200"], ["tuesday"], ["24"], ["120"], ["60000"]]
    res["tasks"]["summary"] = {"score": sum(has(out, k) for k in keys) / len(keys), **st, "out": out}

    hits, stats = 0, []
    for item in QA:
        out, st = gen(model, f"{DOC}\n\nAnswer in one short sentence using only the handbook. Question: {item['q']}", predict=80)
        # "all" answers must every part appear; otherwise any spelling counts
        ok = has(out, item["a"]) if item.get("all") else any(has(out, [a]) for a in item["a"])
        hits += ok; stats.append(st)
    res["tasks"]["qa"] = {"score": hits / len(QA), "avg_ttft_s": round(sum(s["ttft_s"] for s in stats) / len(stats), 3),
                          "gen_tok_s": round(sum(s["gen_tok_s"] for s in stats) / len(stats), 1)}

    schema = {"type": "object", "properties": {
        "actions": {"type": "array", "items": {"type": "object", "properties": {"owner": {"type": "string"}, "task": {"type": "string"}, "due": {"type": "string"}}, "required": ["owner", "task", "due"]}},
        "decisions": {"type": "array", "items": {"type": "string"}}}, "required": ["actions", "decisions"]}
    out, st = gen(model, f"Meeting transcript:\n{TRANSCRIPT}\n\nList every action item (owner, task, due date) and every decision made.", fmt=schema, predict=600)
    try:
        j = json.loads(out)
        acts = [" ".join(str(v) for v in a.values()) for a in j.get("actions", [])]
        decs = " ".join(map(str, j.get("decisions", [])))
        a_score = sum(any(has(a, g) for a in acts) for g in GOLD["actions"]) / len(GOLD["actions"])
        d_score = sum(has(decs, g) for g in GOLD["decisions"]) / len(GOLD["decisions"])
        extra = max(0, len(acts) - len(GOLD["actions"]))
    except Exception:
        a_score = d_score = 0; extra = None
    res["tasks"]["meeting"] = {"actions": a_score, "decisions": d_score, "extra_actions": extra, **st, "out": out}

    out, st = gen(model, f"Clean up this dictation into the text the speaker meant. Remove filler words, apply self-corrections like 'no wait' and 'scratch that'. Output only the cleaned text.\n\n{DICTATION}", predict=120)
    checks = [not re.search(r"\b(um|uh)\b", out.lower()), "sara" in out.lower(), "friday" in out.lower(),
              "tomorrow" not in out.lower(), "thursday" in out.lower()]
    res["tasks"]["dictation"] = {"score": sum(checks) / len(checks), **st, "out": out}

    out, st = gen(model, f"{HINDI}\n\nइस बैठक का सारांश हिंदी में तीन बिंदुओं में लिखें, सभी तारीखें, नाम और बजट शामिल करें।", predict=300)
    checks = [bool(re.search(r"[ऀ-ॿ]{3,}", out)), "15" in out or "पंद्रह" in out, "राहुल" in out, "सीमा" in out,
              ("पाँच" in out or "पांच" in out or "5" in out)]
    res["tasks"]["hindi"] = {"score": sum(checks) / len(checks), **st, "out": out}

    mem.stop = True; mem.join()
    res["peak_rss_mb"] = round(mem.peak / 1024)
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    json.dump(res, open(os.path.join(HERE, "results", re.sub(r"[^\w.-]", "_", model) + ".json"), "w"), indent=1, ensure_ascii=False)
    t = res["tasks"]
    print(f"{model:45s} sum {t['summary']['score']:.2f} qa {t['qa']['score']:.2f} meet {t['meeting']['actions']:.2f}/{t['meeting']['decisions']:.2f} "
          f"dict {t['dictation']['score']:.2f} hi {t['hindi']['score']:.2f} | prompt {t['summary']['prompt_tok_s']} tok/s, gen {t['summary']['gen_tok_s']} tok/s, "
          f"ttft {t['summary']['ttft_s']}s (doc), qa ttft {t['qa']['avg_ttft_s']}s, load {t['summary']['load_s']}s, peak {res['peak_rss_mb']} MB")
    post("/api/generate", {"model": model, "keep_alive": 0}).read()


if __name__ == "__main__":
    for m in sys.argv[1:]:
        run(m)
