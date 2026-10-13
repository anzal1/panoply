#!/usr/bin/env python3
"""Measure the three command bar builds the same way.

usage: python3 bench.py swiftui|tauri|flutter /path/to/App.app
"""
import json, os, statistics, subprocess, sys, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
PORTS = {"swiftui": 47811, "tauri": 47812, "flutter": 47813, "gpui": 47814}


def get(port, path, timeout=30):
    with urllib.request.urlopen(f"http://127.0.0.1:{port}{path}", timeout=timeout) as r:
        return json.loads(r.read() or b"null")


def pids_named(fragment):
    out = subprocess.run(["pgrep", "-f", fragment], capture_output=True, text=True).stdout
    return {int(p) for p in out.split()}


def rss_mb(pids):
    if not pids:
        return 0.0
    out = subprocess.run(["ps", "-o", "rss=", "-p", ",".join(map(str, pids))],
                         capture_output=True, text=True).stdout
    return sum(int(x) for x in out.split()) / 1024


def footprint_mb(pid):
    # phys_footprint is what Activity Monitor shows as Memory
    out = subprocess.run(["footprint", "-p", str(pid)], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if "Footprint:" in line:
            num, unit = line.split("Footprint:")[1].split()[:2]
            return float(num) * (1024 if unit.startswith("G") else 1 if unit.startswith("M") else 1 / 1024)
    return None


def wait_port(port, secs=20):
    end = time.time() + secs
    while time.time() < end:
        try:
            get(port, "/metrics", timeout=1)
            return True
        except Exception:
            time.sleep(0.05)
    return False


def quit_app(app, exe_pids):
    for p in exe_pids:
        try:
            os.kill(p, 15)
        except ProcessLookupError:
            pass
    time.sleep(1.0)


def main():
    build, app = sys.argv[1], sys.argv[2].rstrip("/")
    port = PORTS[build]
    exe_name = os.path.basename(app)[:-4]
    launch_file = os.path.join(HERE, "results", f"{build}-launch.txt")
    res = {"build": build}

    res["bundle_mb"] = round(int(subprocess.run(["du", "-sk", app], capture_output=True, text=True)
                                .stdout.split()[0]) / 1024, 1)

    # cold start: open to first presented frame, 5 runs
    cold = []
    for _ in range(5):
        quit_app(app, pids_named(f"{app}/Contents/MacOS"))
        before = os.path.getsize(launch_file) if os.path.exists(launch_file) else 0
        webkit_before = pids_named("com.apple.WebKit")  # helpers that existed before this launch
        t0 = time.time() * 1000
        subprocess.run(["open", "-g", app])
        end = time.time() + 20
        while time.time() < end:
            if os.path.exists(launch_file) and os.path.getsize(launch_file) > before:
                time.sleep(0.05)
                last = open(launch_file).read().split()[-1]
                cold.append(float(last) - t0)
                break
            time.sleep(0.01)
    res["cold_start_ms"] = {"runs": [round(c, 1) for c in cold],
                            "median": round(statistics.median(cold), 1) if cold else None}

    assert wait_port(port), "control port never came up"

    # show latency: 30 hide/show cycles, read the app's own t0->t1 numbers
    base = len(get(port, "/metrics")["show_ms"])
    for _ in range(30):
        get(port, "/toggle")  # show
        time.sleep(0.35)
        get(port, "/toggle")  # hide
        time.sleep(0.25)
    shows = [s for s in get(port, "/metrics")["show_ms"][base:] if s is not None]
    shows_sorted = sorted(shows)
    res["show_ms"] = {"n": len(shows), "p50": round(statistics.median(shows), 2),
                      "p95": round(shows_sorted[int(len(shows) * 0.95) - 1], 2),
                      "max": round(max(shows), 2)} if shows else None

    # typing to frame
    res["selftest"] = [get(port, "/selftest", timeout=60) for _ in range(3)]
    get(port, "/toggle")

    # idle memory after 30 s, app process plus any WebKit helpers it spawned
    time.sleep(30)
    app_pids = pids_named(f"{app}/Contents/MacOS")
    # WKWebView runs in launchd-spawned helper processes, so count the ones born with the last launch
    helper_pids = pids_named("com.apple.WebKit") - webkit_before if build == "tauri" else set()
    res["idle_memory_mb"] = {
        "app_rss": round(rss_mb(app_pids), 1),
        "app_footprint": [footprint_mb(p) for p in app_pids],
        "helper_pids": sorted(helper_pids),
        "helpers_rss": round(rss_mb(helper_pids), 1),
    }
    quit_app(app, app_pids)

    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    with open(os.path.join(HERE, "results", f"{build}.json"), "w") as f:
        json.dump(res, f, indent=2)
    print(json.dumps(res, indent=2))


if __name__ == "__main__":
    main()
