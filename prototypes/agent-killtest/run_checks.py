#!/usr/bin/env python3
"""Run every task's check.py and print a verdict table. Does not modify anything.
Usage: run_checks.py [ARENA_DIR]   (ARENA_DIR holds <task-name>/{fixtures,work}; default is tasks/)"""
import os, subprocess, sys
ROOT = os.path.dirname(os.path.abspath(__file__))
T = os.path.join(ROOT, "tasks")
arena = sys.argv[1] if len(sys.argv) > 1 else None
for name in sorted(os.listdir(T)):
    c = os.path.join(T, name, "check.py")
    if os.path.exists(c):
        env = dict(os.environ)
        if arena:
            env["KT_TASKDIR"] = os.path.join(arena, name)
        r = subprocess.run([sys.executable, c], capture_output=True, text=True, env=env)
        out = (r.stdout or r.stderr).strip()
        print(f"{name:28s} {out.splitlines()[-1] if out else 'no output'}")
