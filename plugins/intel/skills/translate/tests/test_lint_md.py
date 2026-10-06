#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow>=10"]
# ///
# ABOUTME: Tests lint_md.py: each translator-output defect it reports (lost image tokens, unclosed tags, markup
# ABOUTME: or images inside math, unconverted math), run on a generated work dir.
import json
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "lint_md.py"
fails = []


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


def lint(sections, images=None):
    """Write a work dir with one (source, md) pair per section and return lint_md.py's stdout."""
    with tempfile.TemporaryDirectory() as d:
        work = Path(d)
        (work / "text").mkdir()
        (work / "translated").mkdir()
        meta = {"sections": []}
        for i, (src, md) in enumerate(sections, 1):
            sid = f"{i:02d}"
            (work / "text" / f"{sid}-s.xhtml").write_text(src)
            (work / "translated" / f"{sid}-s.md").write_text(md)
            meta["sections"].append({"id": sid, "title": "S", "file": f"text/{sid}-s.xhtml"})
        (work / "sections.json").write_text(json.dumps(meta))
        if images:
            (work / "images.json").write_text(json.dumps(images))
            (work / "images").mkdir()
            from PIL import Image
            for k, v in images.items():  # a figure is wide; a symbol is a narrow glyph
                Image.new("RGB", (v.get("px", 400), 30), "white").save(work / "images" / v["file"])
        return subprocess.run([sys.executable, str(SCRIPT), str(work)], capture_output=True, text=True).stdout


clean = lint([("<p>见 ⟦IMG:e1⟧ 和 <em>x</em></p>", "# 标题\n\n见 ⟦IMG:e1⟧ 和 \\(x\\)。\n")])
check("a clean section reports nothing", "0 problems" in clean, clean)

# A block image (a figure, a display equation) must survive; an inline symbol image may rightly become LaTeX.
out = lint([("<p>⟦IMG:e1⟧ ⟦IMG:e2⟧ ⟦IMG:e3⟧ ⟦IMG:e4⟧</p>", "# 标题\n\n⟦IMG:e1⟧ ⟦IMG:e9⟧\n")],
           {k: {"file": f"{k}.png", "block": k != "e3"} for k in ("e1", "e2", "e3")}
           | {"e4": {"file": "e4.png", "block": True, "px": 20}})
check("a dropped block image token is reported", "missing ⟦IMG:e2⟧" in out, out)
check("a dropped inline symbol image is not reported", "e3" not in out, out)
check("a dropped narrow symbol marked block is not reported", "e4" not in out, out)
check("an invented image token is reported", "unknown ⟦IMG:e9⟧" in out, out)

out = lint([("<p>x</p>", "# 标题\n\n令 <code>ROW 与后文\n\n下一段。\n")])
check("an unclosed inline tag is reported", "unclosed <code>" in out, out)

out = lint([("<p>x</p>", "# 标题\n\n设 \\(*a* + b\\)，\\(x₁\\)，\\(⟦IMG:e1⟧ + y\\)。\n")])
check("Markdown inside math is reported", "markdown in math" in out, out)
check("a starred superscript is not markdown", "markdown" not in lint([("<p>x</p>", "# 标题\n\n\\(\\det A^{*} = t(G^{*})\\)\n")]))
check("a Unicode subscript inside math is reported", "unicode sub/superscript in math" in out, out)
check("an image token inside math is reported", "image in math" in out, out)

out = lint([("<p>x</p>", "# 标题\n\n设 *n* 为正整数，<em>k</em><sub>1</sub> 为下标。\n")])
check("an italic single letter outside math (unconverted variable) is reported", "unconverted math" in out, out)

out = lint([("<p>x</p>", "# 标题\n\n设 \\(x 未闭合。\n")])
check("unbalanced math delimiters are reported", "unbalanced \\(" in out, out)

out = lint([("<p>⟦CODE:c1⟧</p>", "# 标题\n\n⟦CODE:c1⟧\n\n```\n\\(unclosed <sup>\n```\n")])
check("protected code and fenced contents are not missing or math", "0 problems" in out, out)
out = lint([("<p>⟦CODE:c1⟧ ⟦CODE:c1⟧</p>", "# 标题\n\n⟦CODE:c1⟧ ⟦CODE:c9⟧")])
check("missing and unknown code tokens are reported", "missing ⟦CODE:c1⟧" in out and "unknown ⟦CODE:c9⟧" in out, out)

if fails:
    print(f"\n{len(fails)} failures")
    sys.exit(1)
print("\nall passed")
