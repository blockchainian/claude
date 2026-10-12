#!/usr/bin/env python3
# ABOUTME: Runs the whole scanned-document route on each given PDF, several at once: page images, codex OCR with
# ABOUTME: proofreading, assembly, translation, verbatim restores, render and format check. Resumable per step.
#
# Usage: scan.py <pdf> ... --kind paper|handwritten [--out-dir DIR] [--mirror ROOT] [--jobs 2]
# A work dir per PDF goes under <INTEL_STATE_DIR>/translate-scan/<name>/ with run.log. The PDF goes to
# <out-dir>/<name>.pdf (default out-dir <INTEL_OUTPUT_DIR>/translate); with --mirror ROOT, to <out-dir>/<the PDF's
# path relative to ROOT>, so a course tree keeps its folders. Prints one line per file: ok, or FAILED with the step.
import argparse
import json
import re
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
SKILLS = HERE.parents[1]
TRANSLATE = SKILLS / "translate" / "scripts"
PAPERS = SKILLS / "translate-paper" / "scripts"

# Documents in flight: more parallel renders and translations have run a 16 GB Mac out of memory.
MAX_JOBS = 2

KINDS = {
    # mode for ocr.mjs, page dpi, extra assemble.py options
    "paper": ("typeset", 300, []),
    "handwritten": ("handwritten", 200, ["--one-section", "--ink"]),
}


def intel_paths():
    """The Intel output, state and data roots, as resolved by the plugin's one path helper."""
    script = SKILLS / "fetch-x-mentions" / "scripts" / "env.mjs"
    return {k: Path(v) for k, v in json.loads(subprocess.check_output(["node", str(script)], text=True)).items()}


def title_of(pdf):
    """A notes file's title from its name: "L03-divide-and-conquer-fft-handwritten" -> "Divide and Conquer Fft"."""
    stem = re.sub(r"^[A-Z]+\d+[a-z]?-|-handwritten$", "", pdf.stem)
    small = {"and", "or", "of", "the", "a", "in", "on", "for", "to", "vs"}
    return " ".join(w if i and w in small else w.capitalize() for i, w in enumerate(stem.split("-")))


def steps(pdf, kind, work, out):
    mode, dpi, extra = KINDS[kind]
    title = ["--title", title_of(pdf)] if kind == "handwritten" else []
    return [
        ("pages", ["pdftoppm", "-r", str(dpi), "-png", str(pdf), str(work / "pages" / "p")],
         not any((work / "pages").iterdir())),
        ("ocr", ["node", str(HERE / "ocr.mjs"), str(work), "--mode", mode, "--proofread"], True),
        ("assemble", ["python3", str(HERE / "assemble.py"), str(work), "--source", str(pdf), *title, *extra], True),
        ("translate", ["node", str(PAPERS / "translate_paper.mjs"), str(work)], True),
        ("restore", ["python3", str(HERE / "restore_verbatim.py"), str(work)], True),
        ("lint", [str(TRANSLATE / "lint_md.py"), str(work)], True),
        ("render", [str(PAPERS / "render_paper.py"), str(work), "--out", str(out)], True),
        ("format", [str(TRANSLATE / "format_check.py"), str(out)], True),
    ]


def run_one(pdf, opt):
    work = opt.state / "translate-scan" / pdf.stem
    out = opt.out_dir / (pdf.relative_to(opt.mirror) if opt.mirror else pdf.name)
    (work / "pages").mkdir(parents=True, exist_ok=True)
    out.parent.mkdir(parents=True, exist_ok=True)
    with (work / "run.log").open("a") as log:
        for name, cmd, wanted in steps(pdf, opt.kind, work, out):
            if not wanted:
                continue
            log.write(f"\n== {name}: {' '.join(cmd)}\n")
            log.flush()
            r = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT)
            if r.returncode != 0 and name != "format":  # format findings are reported in the log, not fatal
                return f"FAILED {pdf.name}: step {name} exited {r.returncode} (see {work / 'run.log'})"
    return f"ok {pdf.name} -> {out}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pdfs", nargs="+")
    ap.add_argument("--kind", choices=sorted(KINDS), required=True)
    ap.add_argument("--out-dir", help="where the translated PDFs go (default <INTEL_OUTPUT_DIR>/translate)")
    ap.add_argument("--mirror", help="keep each PDF's folders below this root under --out-dir")
    ap.add_argument("--jobs", type=int, default=MAX_JOBS, help=f"documents in flight, at most {MAX_JOBS}")
    opt = ap.parse_args()
    paths = intel_paths()
    opt.state = paths["state"]
    opt.out_dir = Path(opt.out_dir).expanduser().resolve() if opt.out_dir else paths["output"] / "translate"
    opt.mirror = Path(opt.mirror).expanduser().resolve() if opt.mirror else None
    with ThreadPoolExecutor(max(1, min(opt.jobs, MAX_JOBS))) as pool:
        for line in pool.map(lambda p: run_one(p, opt), [Path(p).expanduser().resolve() for p in opt.pdfs]):
            print(line, flush=True)


if __name__ == "__main__":
    main()
