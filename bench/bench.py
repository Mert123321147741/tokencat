#!/usr/bin/env python3
"""Token savings benchmark on real open-source test suites.

Clones each project at a pinned commit, applies a small bug from
bench/patches/, runs the project's own test command the way an agent would
(no TTY, stdout+stderr combined), then pipes that output through tokencat
and records the token counts tokencat reports.

    python3 bench/bench.py --tokencat target/release/tokencat --work /tmp/tc-bench

Requires git, python3 >= 3.10 (pip), node/npm, go and cargo on PATH.
"""

import argparse
import os
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent

PROJECTS = {
    "click": ("https://github.com/pallets/click", "06b2a678741131fd577ce170e23e5ca0aeba0309"),
    "cobra": ("https://github.com/spf13/cobra", "adbc8813901bba65827259daa8e22ff94ec1f30e"),
    "semver": ("https://github.com/dtolnay/semver", "280ebcb6edac3aa4cdc545dbff8a26c5ac4861fe"),
    "ms": ("https://github.com/vercel/ms", "4ff48cec099f0514c3e9bbca18706c9c21122bfb"),
    "ufo": ("https://github.com/unjs/ufo", "f06c800d0c59f2a4a1b9ba65eb6cb61a84419be6"),
    "django": ("https://github.com/django/django", "e802ada38b3ecf345915163bb6d7f008be411664"),  # 5.2.17
}

# (label, project, command)
SCENARIOS = [
    ("click · pytest", "click", "{venv}/bin/python -m pytest -p no:cacheprovider"),
    ("click · pytest -v", "click", "{venv}/bin/python -m pytest -v -p no:cacheprovider"),
    ("click · pytest -v (all pass)", "click", "{venv}/bin/python -m pytest -v -p no:cacheprovider tests/test_basic.py"),
    ("cobra · go test ./...", "cobra", "go test ./..."),
    ("cobra · go test -v ./...", "cobra", "go test -v ./..."),
    ("semver · cargo test", "semver", "cargo test"),
    ("ms · jest", "ms", "npx jest --env node"),
    ("ufo · vitest run", "ufo", "npx vitest run"),
    ("django · runtests.py utils_tests", "django", "cd tests && {djvenv}/bin/python runtests.py --parallel 1 utils_tests"),
    ("django · runtests.py -v 2 utils_tests", "django", "cd tests && {djvenv}/bin/python runtests.py --parallel 1 -v 2 utils_tests"),
]


def sh(cmd, cwd=None, check=True):
    r = subprocess.run(cmd, shell=True, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if check and r.returncode != 0:
        sys.exit(f"command failed: {cmd}\n{r.stdout.decode(errors='replace')[-2000:]}")
    return r


def setup(work: Path):
    work.mkdir(parents=True, exist_ok=True)
    for name, (url, rev) in PROJECTS.items():
        d = work / name
        if not d.exists():
            sh(f"git init -q {d} && git -C {d} remote add origin {url}")
            sh(f"git -C {d} fetch -q --depth 1 origin {rev} && git -C {d} checkout -q FETCH_HEAD")
        sh(f"git -C {d} checkout -q -- . && git -C {d} apply {HERE / 'patches' / (name + '.patch')}")
    venv = work / "venv"
    if not venv.exists():
        sh(f"python3 -m venv {venv} && {venv}/bin/pip install -q pytest -e {work / 'click'}")
    djvenv = work / "djvenv"
    if not djvenv.exists():
        sh(f"python3 -m venv {djvenv} && {djvenv}/bin/pip install -q -e {work / 'django'}")
    for js in ("ms", "ufo"):
        if not (work / js / "node_modules").exists():
            sh("npm install --no-audit --no-fund --ignore-scripts --legacy-peer-deps", cwd=work / js)
    # Warm builds so compile progress is not part of the measured output.
    sh("go test ./... >/dev/null 2>&1 || true", cwd=work / "cobra", check=False)
    sh("cargo test --no-run -q", cwd=work / "semver", check=False)
    return {"venv": venv, "djvenv": djvenv}


FOOTER = re.compile(r"\[tokencat: ([\d,]+) -> ([\d,]+) tokens")


def measure(tokencat, label, cwd, cmd):
    raw = sh(cmd, cwd=cwd, check=False).stdout
    t = time.perf_counter()
    out = subprocess.run(
        [tokencat, "--no-log"], input=raw, cwd=cwd, stdout=subprocess.PIPE
    ).stdout.decode(errors="replace")
    ms = (time.perf_counter() - t) * 1000
    m = FOOTER.search(out)
    if m:
        before, after = (int(x.replace(",", "")) for x in m.groups())
    else:  # output too small to be worth a footer: count it ourselves
        n = subprocess.run([tokencat, "--raw"], input=raw, stdout=subprocess.PIPE).stdout
        before = after = len(n) // 4
    lines_before = raw.decode(errors="replace").count("\n")
    lines_after = out.count("\n")
    return {
        "label": label,
        "lines": (lines_before, lines_after),
        "tokens": (before, after),
        "ms": ms,
        "output": out,
        "raw": raw,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tokencat", default=str(HERE.parent / "target/release/tokencat"))
    ap.add_argument("--work", default="/tmp/tokencat-bench")
    ap.add_argument("--save", help="directory to save raw and pruned outputs")
    args = ap.parse_args()
    tokencat = os.path.abspath(args.tokencat)
    work = Path(args.work)
    envs = setup(work)

    rows = []
    for label, project, cmd in SCENARIOS:
        r = measure(tokencat, label, work / project, cmd.format(**envs))
        rows.append(r)
        if args.save:
            out = Path(args.save)
            out.mkdir(parents=True, exist_ok=True)
            slug = re.sub(r"[^a-z0-9]+", "-", label.lower()).strip("-")
            (out / f"{slug}.raw.log").write_bytes(r["raw"])
            (out / f"{slug}.tokencat.md").write_text(r["output"])

    print("| Scenario | Lines | Tokens (est.) | Saved | Wall time |")
    print("|---|---:|---:|---:|---:|")
    tb = ta = 0
    for r in rows:
        (lb, la), (b, a) = r["lines"], r["tokens"]
        tb, ta = tb + b, ta + a
        pct = (b - a) * 100 / b if b else 0
        print(f"| {r['label']} | {lb:,} → {la:,} | {b:,} → {a:,} | {pct:.1f}% | {r['ms']:.1f} ms |")
    print(f"| **Total** | | **{tb:,} → {ta:,}** | **{(tb - ta) * 100 / tb:.1f}%** | |")


if __name__ == "__main__":
    main()
