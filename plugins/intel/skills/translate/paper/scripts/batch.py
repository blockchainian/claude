#!/usr/bin/env python3
# ABOUTME: Runs the paper pipeline over a list of PDFs: MinerU conversion one at a time, codex translation and
# ABOUTME: rendering in a pool, smallest files first; resumable, one log per file and a status table.
#
# Usage: batch.py <list>... --out-root <dir> [--state <dir>] [--translate 4] [--jobs 10] [--only <substring>]
# A list holds one PDF path per line (a tab-separated line uses its last field). Each PDF's output is
# <out-root>/<its folder name>/<its stem>.pdf; its work dir is <state>/translate-paper/<folder key>-<stem>/, the
# key being the course number that opens the folder name (else the folder name). A file whose output exists is
# skipped; delete the output to redo it.
import argparse
import json
import os
import queue
import re
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
lock = threading.Lock()


def state_root():
    env = HERE.parents[2] / "fetch-x-mentions" / "scripts" / "env.mjs"
    return Path(json.loads(subprocess.check_output(["node", str(env)], text=True))["state"])


def page_count(pdf):
    out = subprocess.run(["pdfinfo", str(pdf)], capture_output=True, text=True).stdout
    m = re.search(r"^Pages:\s+(\d+)", out, re.M)
    return int(m.group(1)) if m else 1


def run(cmd, log, timeout=None):
    """Runs a stage with its output appended to the file's log; a stage past its timeout is killed (exit 124)."""
    with log.open("a") as f:
        f.write(f"\n$ {' '.join(map(str, cmd))}\n")
        f.flush()
        # its own process group, so a timeout kills the whole tree (uv run's python child and MinerU under it),
        # not just the uv launcher, which would leave the workers running
        proc = subprocess.Popen(list(map(str, cmd)), stdout=f, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            return proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
            f.write(f"\n[timed out after {timeout}s]\n")
            return 124


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("lists", nargs="+")
    ap.add_argument("--out-root", required=True, help="outputs go to <out-root>/<source folder name>/<stem>.pdf")
    ap.add_argument("--state", help="state root (default <INTEL_STATE_DIR>)")
    ap.add_argument("--translate", type=int, default=2, help="files translated and rendered at once (each render runs a Chrome)")
    ap.add_argument("--jobs", type=int, default=10, help="codex calls in flight per file")
    ap.add_argument("--only", help="only files whose path contains this")
    opt = ap.parse_args()
    out_root = Path(opt.out_root).expanduser()
    work_root = (Path(opt.state).expanduser() if opt.state else state_root()) / "translate-paper"
    work_root.mkdir(parents=True, exist_ok=True)
    status = work_root / "status.tsv"

    def record(name, stage, detail=""):
        with lock:
            with status.open("a") as f:
                f.write(f"{time.strftime('%F %T')}\t{name}\t{stage}\t{detail}\n")
            print(f"{name}\t{stage}\t{detail}", flush=True)

    def job(pdf):
        folder = pdf.parent.name
        key = (re.match(r"\d+(\.\d+)*", folder) or re.match(r".*", folder)).group(0)
        return work_root / f"{key}-{pdf.stem}", out_root / folder / f"{pdf.stem}.pdf", f"{key}/{pdf.stem}"

    todo = queue.Queue()
    files = []
    for lst in opt.lists:
        for line in open(lst):
            path = line.rstrip("\n").split("\t")[-1].strip()
            if path and (not opt.only or opt.only in path):
                files.append(Path(path))
    for pages, pdf in sorted((page_count(p), p) for p in files):
        work, out, name = job(pdf)
        if out.exists():
            record(name, "skip", "output exists")
        else:
            todo.put((pages, pdf))
    converted = queue.Queue()

    def converter():
        while True:
            try:
                pages, pdf = todo.get_nowait()
            except queue.Empty:
                return
            work, out, name = job(pdf)
            work.mkdir(parents=True, exist_ok=True)
            log = work / "batch.log"
            t0 = time.time()
            # MinerU sometimes hangs at start-up with no output (two runs side by side did); a run is killed past
            # 60 s + 15 s per page and tried once more
            limit = 60 + 15 * pages
            rc = run([HERE / "pdf2work.py", pdf, work], log, limit)
            if rc == 124:
                rc = run([HERE / "pdf2work.py", pdf, work], log, limit)
            record(name, "converted" if rc == 0 else "FAILED convert", f"{pages}p {time.time() - t0:.0f}s")
            if rc == 0:
                converted.put((pages, pdf))

    def translator():
        while True:
            item = converted.get()
            if item is None:
                return
            pages, pdf = item
            work, out, name = job(pdf)
            log = work / "batch.log"
            t0 = time.time()
            rc = run(["node", HERE / "translate_paper.mjs", work, "--jobs", opt.jobs], log)
            if rc != 0:  # one retry pass for sections that failed the checks twice
                rc = run(["node", HERE / "translate_paper.mjs", work, "--jobs", opt.jobs], log)
            if rc != 0:
                record(name, "FAILED translate", f"see {log}")
                continue
            out.parent.mkdir(parents=True, exist_ok=True)
            rc = run([HERE / "render_paper.py", work, "--out", out], log)
            if rc != 0:  # headless Chrome now and then exits non-zero while other renders run; a KaTeX error repeats
                rc = run([HERE / "render_paper.py", work, "--out", out], log)
            if rc != 0 and out.exists():  # render.py writes the PDF before failing; keep it out of the outputs
                out.replace(work / "render-failed.pdf")
            record(name, "done" if rc == 0 else "FAILED render", f"{pages}p {time.time() - t0:.0f}s {out if rc == 0 else log}")

    conv = threading.Thread(target=converter)
    trans = [threading.Thread(target=translator) for _ in range(opt.translate)]
    for t in [conv] + trans:
        t.start()
    conv.join()
    for _ in trans:
        converted.put(None)
    for t in trans:
        t.join()
    record("batch", "finished")


if __name__ == "__main__":
    sys.exit(main())
