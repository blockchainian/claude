#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pymupdf", "pdfplumber", "numpy"]
# ///
# ABOUTME: Checks opener detection against generated PDFs with captions before prose.
import subprocess
import sys
import tempfile
from pathlib import Path

import pymupdf

with tempfile.TemporaryDirectory() as d:
    pdf = pymupdf.open()
    page = pdf.new_page(width=468, height=680)
    page.insert_text((100, 100), "S04", fontsize=1)
    page.insert_text((82, 130), "图　示意", fontname="china-s", fontsize=9.25)
    # The opening image occupies the remainder of this page; prose and its cap start on the next page.
    page.draw_rect(pymupdf.Rect(65, 150, 400, 620), fill=(0.8, 0.8, 0.8))
    page = pdf.new_page(width=468, height=680)
    page.insert_text((64, 90), "正", fontname="china-s", fontsize=24)
    page.insert_text((90, 85), "文从这里开始。", fontname="china-s", fontsize=9.25)
    path = Path(d) / "caption.pdf"
    pdf.save(path)
    result = subprocess.run([sys.executable, str(Path(__file__).resolve().parent.parent / "scripts" / "format_check.py"), str(path)], capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    assert "dropcap" not in result.stdout.split("problems:")[0], result.stdout
    print("PASS: unnumbered caption page does not require a drop cap")
