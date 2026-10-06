#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pymupdf", "pdfplumber", "numpy"]
# ///
# ABOUTME: Checks a rendered book PDF for layout defects, reading only the PDF: missing drop caps, slanted Chinese,
# ABOUTME: bold Chinese off the baseline, loose lines, marker-pushed exercise numbers, list sections' first items.
"""Usage: format_check.py <book.pdf>  -- one line per problem (kind, page and section, detail), then a summary."""
import collections, logging, re, statistics, sys
import numpy as np
import pdfplumber
import pymupdf

from openers import CAPTION_TEXT_RE

logging.getLogger("pdfminer").setLevel(logging.ERROR)

pdf_path = sys.argv[1]
pdf = pymupdf.open(pdf_path)
toc = [(t, p) for _, t, p in pdf.get_toc()]
problems = []


def section_of(pageno):
    return max(((t, p) for t, p in toc if p <= pageno), key=lambda x: x[1], default=("?", 0))[0]


def where(pageno):
    return f"p{pageno} {section_of(pageno)}"


def page_lines(page, mode="dict"):
    """The page's text lines, top to bottom. Spans whose baselines are within 1.5pt are one visual line: a glyph
    nudged off the baseline (bold Chinese) otherwise splits a line in two."""
    spans = [sp for b in page.get_text(mode)["blocks"] for l in b.get("lines", []) for sp in l["spans"]]
    rows = []
    for sp in sorted(spans, key=lambda sp: sp["origin"][1]):
        if rows and abs(sp["origin"][1] - rows[-1][0]) <= 1.5:
            rows[-1][1].append(sp)
        else:
            rows.append((sp["origin"][1], [sp]))
    lines = []
    for _, row in rows:
        row.sort(key=lambda sp: sp["bbox"][0])
        x0, y0, x1, y1 = zip(*(sp["bbox"] for sp in row))
        lines.append({"spans": row, "bbox": (min(x0), min(y0), max(x1), max(y1))})
    return lines


def line_text(line):
    return "".join(sp["text"] for sp in line["spans"]).strip()


def is_bold(span):
    # Body text is Songti / Baskerville regular; bold Chinese comes out as Chrome's Type3 黑体 glyphs.
    return "Bold" in span["font"] or span["font"].startswith("Type3")


# Slanted Chinese: Songti has no italic, so Chrome fakes one by skewing the glyph; the skew shows in the char
# matrix. The slanted lines' y positions are kept so the drop-cap check can skip an epigraph.
CJK = re.compile(r"[\u4e00-\u9fff]")
slanted_y = collections.defaultdict(set)
with pdfplumber.open(pdf_path) as doc:
    for i, page in enumerate(doc.pages):
        chars = [c for c in page.chars if abs(c["matrix"][2]) > 0.01 and re.match(r"[一-鿿]", c["text"])]
        if chars:
            slanted_y[i] = {round(c["top"]) for c in chars}
            problems.append((where(i + 1), "italic", f"{len(chars)} chars: {''.join(c['text'] for c in chars)[:30]}"))
        # Loose lines: justification stretches the space between Chinese characters when a long Latin run (a title,
        # a name) cannot break. Set solid, adjacent CJK glyphs touch; a median gap over 1pt is visibly spaced out.
        rows = collections.defaultdict(list)
        for c in page.chars:
            if 8.5 < c["size"] < 10.5:
                rows[round(c["top"])].append(c)
        loose = []
        for row in rows.values():
            row.sort(key=lambda c: c["x0"])
            gaps = [b["x0"] - a["x1"] for a, b in zip(row, row[1:]) if CJK.match(a["text"]) and CJK.match(b["text"])]
            if len(gaps) >= 5 and statistics.median(gaps) > 1:
                loose.append("".join(c["text"] for c in row))
        if loose:
            problems.append((where(i + 1), "loose", f"{len(loose)} lines: {loose[0][:30]}"))


def slanted(pageno, line):
    y0, y1 = line["bbox"][1] - 1, line["bbox"][3] + 1
    return any(y0 <= y <= y1 for y in slanted_y.get(pageno, ()))


# Bold Chinese riding high: bold comes from a different face (黑体) whose glyphs can sit higher on the shared
# baseline than Songti's. Compare the vertical centre of each glyph's ink on the page, bold against regular, on
# lines that mix both; a gap over 0.3pt is visible.
ZOOM = 4
shifts = {}
for page in pdf:
    mixed = []
    for l in page_lines(page, "rawdict"):
        cs = [(c, is_bold(sp)) for sp in l["spans"] if 8.5 < sp["size"] < 10.5 for c in sp["chars"] if CJK.match(c["c"])] \
            if "chars" in l["spans"][0] else []
        if any(b for _, b in cs) and sum(1 for _, b in cs if not b) > 3:
            mixed.append(cs)
    if not mixed:
        continue
    pix = page.get_pixmap(matrix=pymupdf.Matrix(ZOOM, ZOOM), colorspace=pymupdf.csGRAY)
    img = np.frombuffer(pix.samples, np.uint8).reshape(pix.height, pix.stride)[:, :pix.width].astype(int)
    ink = np.abs(img - img[0, 0]) > 40

    def centre(c):
        x0, x1 = int(c["bbox"][0] * ZOOM), int(c["bbox"][2] * ZOOM)
        y0, y1 = int((c["origin"][1] - 12) * ZOOM), int((c["origin"][1] + 4) * ZOOM)
        rows = np.nonzero(ink[max(y0, 0):y1, x0:x1].any(axis=1))[0]
        return (rows[0] + rows[-1]) / 2 / ZOOM + y0 / ZOOM if len(rows) else None

    diffs = []
    for cs in mixed:
        bold = [v for c, b in cs if b for v in [centre(c)] if v is not None]
        reg = [v for c, b in cs if not b for v in [centre(c)] if v is not None]
        if bold and reg:
            diffs.append(statistics.median(reg) - statistics.median(bold))
    if diffs and statistics.median(diffs) > 0.3:
        shifts[page.number + 1] = statistics.median(diffs)
if shifts:
    pages = sorted(shifts)
    problems.append((f"{len(pages)} pages", "bold-high", f"bold Chinese sits {statistics.median(shifts.values()):.2f}pt above the text: "
                     f"p{', p'.join(map(str, pages[:12]))}{' ...' if len(pages) > 12 else ''}"))

# Column geometry: render.py sets the side margins to 13.5% of the page width; the indent is 2em further in.
sizes = [sp["size"] for page in pdf for l in page_lines(page) for sp in l["spans"] if 8.5 < sp["size"] < 10.5]
margin = pdf[0].rect.width * 0.135
right = pdf[0].rect.width - margin
indent = margin + 2 * statistics.median(sizes) if sizes else margin

# Exercise numbers: a bold "N." opening a line should start at the paragraph indent like the others. A ▶ marker
# image set inline in front of the number pushes it right instead of hanging
# in the margin.
off = collections.defaultdict(list)
for page in pdf:
    for l in page_lines(page):
        first = next((sp for sp in l["spans"] if sp["text"].strip()), None)
        if first and "Bold" in first["font"] and re.fullmatch(r"\d+\.", first["text"].strip()):
            if 3 < first["bbox"][0] - indent < 40:  # pushed right by an inline marker; not a wrapped line or mid-line
                off[page.number + 1].append(first["text"].strip())
for pg, nums in off.items():
    problems.append((where(pg), "exercise", f"number pushed right of the indent: {' '.join(nums)}"))


# Section openers: the ⟦S<id>⟧ marker (about 1pt) sits in each section's title. Below it, the first body line
# under the title says how the section opens; a span at >= 23pt is the drop cap. An epigraph (slanted, or inset
# 2em from both edges of the column) comes before the text and is skipped.
def kind(text, span):
    text = text.lstrip("▶▸ ")  # a ▶ marker set as text, hung in front of an exercise number
    if re.match(r"(\d+\.(?!\d)|表\s*\d|图\s*\d|算法\s*\d)", text):
        return "label"  # an answer "1.", "表 1", "算法 1.1E"
    if text[:1].isdigit():
        return "digit"
    return "bold" if is_bold(span) else "prose"


def quoted(pageno, block):
    """True for an epigraph: its lines are slanted, or it has two or more lines, all inset from both edges of the
    column (a single short line at the indent is as likely a one-line paragraph)."""
    return any(slanted(pageno, l) for l in block) or len(block) > 1 and all(
        l["bbox"][0] >= margin + 12 and l["bbox"][2] <= right - 12 for l in block)


for page in pdf:
    pg = page.number + 1
    lines = page_lines(page)
    marker = next((l for l in lines for sp in l["spans"] if re.fullmatch(r"S\d+", sp["text"].strip()) and sp["size"] < 2), None)
    if marker is None:
        continue
    lines = [l for l in lines if l["bbox"][1] > marker["bbox"][3]]  # below the title: not the "第一章" label above it
    cap = next((sp for l in lines for sp in l["spans"] if sp["size"] >= 23 and sp["text"].strip()), None)
    body = [l for l in lines if any(8.5 < sp["size"] < 10.5 and sp["text"].strip() for sp in l["spans"])]
    # Group lines into blocks by vertical gaps (a paragraph break inside a block is under a line height).
    blocks = []
    for l in body:
        if blocks and l["bbox"][1] - blocks[-1][-1]["bbox"][3] < 8:
            blocks[-1].append(l)
        else:
            blocks.append([l])
    prose = [l for blk in blocks if not quoted(page.number, blk) and not CAPTION_TEXT_RE.match(line_text(blk[0])) for l in blk]
    if not prose:
        continue
    first = prose[0]
    text = (cap["text"].strip() if cap else "") + line_text(first)
    k = kind(text, cap or next(sp for sp in first["spans"] if sp["text"].strip()))
    if cap is None and k in ("prose", "bold"):
        problems.append((where(pg), "dropcap", f"missing ({k}): {text[:30]!r}"))
    elif cap is not None and k in ("label", "digit"):
        problems.append((where(pg), "dropcap", f"on a {k}: {text[:30]!r}"))
    elif cap is not None and k == "bold" and not is_bold(cap):
        problems.append((where(pg), "dropcap", f"not bold, but the first phrase is bold: {text[:30]!r}"))
    # Without a drop cap the first paragraph should be indented like the rest; at the margin, a list section's
    # first item (an answer "1.", an index entry) is out of line with the items below it.
    elif cap is None and abs(first["bbox"][0] - margin) <= 3 and any(abs(l["bbox"][0] - indent) <= 3 for l in prose[1:]):
        problems.append((where(pg), "first-line", f"first item at the margin, the rest indented: {text[:30]!r}"))

for name, what, detail in problems:
    print(f"{what:<10} {name:<40} {detail}")
print(f"\n{len(problems)} problems: " + ", ".join(
    f"{w} {sum(1 for p in problems if p[1] == w)}" for w in ("dropcap", "italic", "bold-high", "loose", "exercise", "first-line")))
