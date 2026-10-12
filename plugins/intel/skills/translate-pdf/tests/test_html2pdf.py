#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pymupdf>=1.24"]
# ///
# ABOUTME: Tests html2pdf.py end to end on a real translated page with headless Chrome: page size, dark background,
# ABOUTME: the Chinese body face and size, 黑体 headings, Menlo code, and --skip-bookmark.
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pymupdf

HERE = Path(__file__).resolve().parent
SCRIPT = HERE.parent / "scripts" / "html2pdf.py"
fails = []


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


with tempfile.TemporaryDirectory() as d:
    page = Path(d) / "page.html"
    shutil.copy(HERE / "fixtures" / "translated-page.html", page)
    r = subprocess.run([str(SCRIPT), str(page), "--base", "https://pdos.csail.mit.edu/6.1810/2026/homework/",
                        "--skip-bookmark", "xv6 阅读作业"], capture_output=True, text=True, timeout=300)
    check("prints OK", r.returncode == 0 and r.stdout.startswith("OK"), r.stdout + r.stderr[-500:])
    doc = pymupdf.open(page.with_suffix(".pdf"))
    p = doc[0]
    check("page is 427.92 x 660 pt", (round(p.rect.width, 2), round(p.rect.height)) == (427.92, 660), str(p.rect))
    # PingFang embeds as a Type 3 font with no base name, so the faces are read from pdffonts.
    fonts = subprocess.run(["pdffonts", "-l", "1", str(page.with_suffix(".pdf"))], capture_output=True, text=True).stdout
    check("Songti body, PingFang headings, Menlo code", all(f in fonts for f in ("Songti", "PingFang", "Menlo")), fonts)
    sizes = [round(s["size"], 2) for b in p.get_text("dict")["blocks"] for l in b.get("lines", []) for s in l["spans"]
             if "Songti" in s["font"]]
    check("body at 9.25pt", max(set(sizes), key=sizes.count) == 9.25, str(set(sizes)))
    pix = p.get_pixmap(clip=pymupdf.Rect(2, 2, 6, 6))
    check("dark background", pix.pixel(0, 0) == (0, 4, 9), str(pix.pixel(0, 0)))
    check("--skip-bookmark drops the entry", all(t != "xv6 阅读作业" for _, t, _ in doc.get_toc()), str(doc.get_toc()))

print(f"{len(fails)} failed" if fails else "all passed")
sys.exit(1 if fails else 0)
