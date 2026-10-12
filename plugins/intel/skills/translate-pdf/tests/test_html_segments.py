#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["beautifulsoup4>=4.12", "lxml>=5"]
# ///
# ABOUTME: Tests html_segments.py on real pages: site chrome and licence badges are dropped, code and math are
# ABOUTME: protected, --drop-heading removes a section, and join restores every tag's attributes.
import json
import subprocess
import sys
import tempfile
from pathlib import Path

from bs4 import BeautifulSoup

HERE = Path(__file__).resolve().parent
SCRIPT = HERE.parent / "scripts" / "html_segments.py"
FIX = HERE / "fixtures"
fails = []


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


def split(page, d, *extra):
    subprocess.run([str(SCRIPT), "split", str(page), str(d / "segs.json"), str(d / "skel.html"), *extra],
                   check=True, capture_output=True)
    return json.loads((d / "segs.json").read_text()), (d / "skel.html").read_text()


with tempfile.TemporaryDirectory() as d:
    d = Path(d)
    segs, skel = split(FIX / "course-page.html", d)
    texts = " ".join(s["text"] for s in segs)
    check("navigation is not sent", "Schedule" not in texts and "navbar" not in skel)
    check("licence badge removed", "creativecommons" not in skel.lower())
    check("footer removed", "<footer" not in skel)
    ul = next(s for s in segs if "<tt>exec.c:loadseg()</tt>" in s["text"])
    check("code kept whole", "<tt>exec.c:loadseg()</tt>" in ul["keep"] and "<tt>pa</tt>" in ul["keep"])
    intro = next(s for s in segs if "Throughout this semester" in s["text"])
    check("attributes become k=n", 'k="0"' in intro["keep"] and "github.com" not in intro["text"])
    check("no math on a page without MathJax", all(not s["math"] for s in segs))

    # Join swaps k="n" back for the original attributes, wherever the translator moved the tag.
    zh = {intro["id"]: '在整个学期中，你需要阅读 <a k="1">xv6 教材</a>，以及 <a k="0">xv6 内核</a>的完整实现。'}
    (d / "zh.json").write_text(json.dumps(zh, ensure_ascii=False))
    subprocess.run([str(SCRIPT), "join", str(d / "skel.html"), str(d / "zh.json"), str(d / "out.html")], check=True,
                   capture_output=True)
    out = BeautifulSoup((d / "out.html").read_text(), "html.parser")
    links = {a.get_text(): a.get("href") for a in out.find_all("a") if a.get_text() in ("xv6 教材", "xv6 内核")}
    check("join restores hrefs", links == {"xv6 教材": "https://mit-pdos.github.io/xv6-riscv-book/",
                                           "xv6 内核": "https://github.com/mit-pdos/xv6-riscv/tree/riscv/kernel"}, str(links))
    check("no data-tid left", "data-tid" not in str(out))

    segs, _ = split(FIX / "mathjax-page.html", d)
    check("MathJax page with $ config protects $..$", any(s["dollar"] for s in segs))
    formulas = [f for s in segs for f in s["math"]]
    check("inline and display formulas found", "$X$" in formulas and any(f.startswith("\\begin{equation}") for f in formulas))
    texts = " ".join(s["text"] for s in segs)
    check("section present before dropping", "Data Compression" in texts)
    segs, _ = split(FIX / "mathjax-page.html", d, "--drop-heading", "data compression")
    texts = " ".join(s["text"] for s in segs)
    check("--drop-heading removes the section", "Data Compression" not in texts and "Encoding Messages" in texts)

print(f"{len(fails)} failed" if fails else "all passed")
sys.exit(1 if fails else 0)
