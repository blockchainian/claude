#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pymupdf", "pikepdf", "fonttools", "markdown", "pillow", "numpy"]
# ///
# ABOUTME: Translates a slide deck whose pages are pictures (no text layer) into Chinese page for page, from OCR text
# boxes (translate-scan's slide_boxes JSON): recolours each page picture for the dark theme, paints over each
# translated box and sets the fitted Chinese in its place. Shares blocks.json, translation and typesetting with
# slides.py.
# Usage: ocr_slides.py extract <deck.pdf> <boxes.json> <work dir>
#        ocr_slides.py render  <deck.pdf> <work dir> <out.pdf>
import html
import json
import os
import re
import sys
from collections import Counter

import pymupdf

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import slides  # noqa: E402  (theme, recolouring and typesetting shared with text-layer decks)

GREEK = {"alpha": "α", "beta": "β", "gamma": "γ", "delta": "δ", "Delta": "Δ", "epsilon": "ε", "varepsilon": "ε",
         "theta": "θ", "lambda": "λ", "mu": "μ", "pi": "π", "rho": "ρ", "sigma": "σ", "Sigma": "Σ", "tau": "τ",
         "phi": "φ", "omega": "ω", "Omega": "Ω"}
SYMBOLS = {"le": "≤", "leq": "≤", "ge": "≥", "geq": "≥", "ne": "≠", "neq": "≠", "times": "×", "cdot": "·",
           "to": "→", "rightarrow": "→", "leftarrow": "←", "Rightarrow": "⇒", "infty": "∞", "approx": "≈",
           "pm": "±", "ldots": "…", "cdots": "⋯", "dots": "…", "sim": "∼", "neg": "¬", "oplus": "⊕", "wedge": "∧",
           "vee": "∨", "lfloor": "⌊", "rfloor": "⌋", "lceil": "⌈", "rceil": "⌉", "log": "log", "quad": " "}


def math_html(tex):
    """Simple inline LaTeX as HTML: Greek and common symbols as Unicode, _ and ^ as sub and superscripts, \\text and
    \\mathrm as plain text. Anything else keeps its letters without the backslash."""
    s = html.escape(tex.strip(), quote=False)
    s = re.sub(r"\\(?:text|mathrm|mathit|mathbf|operatorname)\{([^{}]*)\}", r"\1", s)
    s = re.sub(r"\\([A-Za-z]+)", lambda m: GREEK.get(m.group(1), SYMBOLS.get(m.group(1), m.group(1))), s)
    for _ in range(3):  # nested scripts
        s = re.sub(r"_\{([^{}]*)\}", r"<sub>\1</sub>", s)
        s = re.sub(r"\^\{([^{}]*)\}", r"<sup>\1</sup>", s)
    s = re.sub(r"_(\S)", r"<sub>\1</sub>", s)
    s = re.sub(r"\^(\S)", r"<sup>\1</sup>", s)
    return s.replace("{", "").replace("}", "").replace("\\", "")


def box_text(text):
    """A box's Markdown/LaTeX transcription as block text: math becomes ⟦En⟧ tokens (returned with their HTML) and
    **bold** or *emphasis* becomes an <s1> run, set in 黑体 as Chinese emphasis is."""
    math = {}

    def token(m):
        math[str(len(math) + 1)] = math_html(m.group(1) or m.group(2) or m.group(3))
        return "⟦E%d⟧" % len(math)
    t = re.sub(r"\\\((.+?)\\\)|\\\[(.+?)\\\]|\$([^$]+)\$", token, text, flags=re.S)
    t = html.escape(t, quote=False)
    t = re.sub(r"\*\*(.+?)\*\*|\*(.+?)\*", lambda m: "<s1>%s</s1>" % (m.group(1) or m.group(2)), t)
    t = re.sub(r"\s*\n\s*[-•]\s*", "<br>• ", t)  # Markdown list items keep one per line
    return re.sub(r"\s+", " ", t).strip(), math


def colors(pix, rect, scale):
    """The background and ink colours inside a box of a page pixmap rendered at scale: the commonest colour, and
    the commonest of the colours clearly unlike it."""
    x0, y0 = max(0, int(rect.x0 * scale)), max(0, int(rect.y0 * scale))
    x1, y1 = min(pix.width, int(rect.x1 * scale)), min(pix.height, int(rect.y1 * scale))
    counts = Counter()
    for y in range(y0, y1):
        row = pix.samples[(y * pix.width + x0) * pix.n:(y * pix.width + x1) * pix.n]
        counts.update(tuple(v >> 3 << 3 for v in row[i:i + 3]) for i in range(0, len(row), pix.n))
    if not counts:
        return (255, 255, 255), (0, 0, 0)
    bg = counts.most_common(1)[0][0]
    ink = next((c for c, _ in counts.most_common() if sum(abs(a - b) for a, b in zip(c, bg)) > 180), (0, 0, 0))
    return bg, ink


def extract(deck, boxes_json, work):
    """Writes blocks.json in slides.py's shape from the OCR boxes: one block per matched box outside figures, its
    ink colour and size measured from the page picture, headings (1.3 times the median size of the page's translated
    boxes) bold."""
    doc = pymupdf.open(deck)
    data = json.load(open(boxes_json))
    blocks, skipped = [], 0
    for p in data["pages"]:
        page = doc[p["index"]]
        boxes = [b for b in p["boxes"] if b["matched"] and not b["in_figure"] and re.search(r"[A-Za-z]{2}", b["text"])]
        skipped += len(p["boxes"]) - len(boxes)
        if not boxes or slides.is_ocw_notice(page):
            continue
        scale = 1.0  # 72 dpi is enough to measure colours
        pix = page.get_pixmap(matrix=pymupdf.Matrix(scale, scale))
        median = sorted(b["font_size"] for b in boxes)[len(boxes) // 2]
        for b in boxes:
            r = pymupdf.Rect(b["bbox"])
            _, ink = colors(pix, r, scale)
            text, math = box_text(b["text"])
            lines = [pymupdf.Rect(l["bbox"]) for l in b.get("lines", [])] or [r]
            xs0, xs1 = [l.x0 for l in lines], [l.x1 for l in lines]
            align = "left"
            if len(lines) > 1 and max(xs0) - min(xs0) > 4:
                mids = [(a + c) / 2 for a, c in zip(xs0, xs1)]
                align = "center" if max(mids) - min(mids) < 6 else ("right" if max(xs1) - min(xs1) < 4 else "left")
            size = round(b["font_size"], 1)  # Vision's line-box height matches the type size (median ratio 1.00)
            pitch = (lines[-1].y0 - lines[0].y0) / (len(lines) - 1) / size if len(lines) > 1 else None
            base = {"size": size, "color": "#%02x%02x%02x" % ink, "bold": b["font_size"] >= 1.3 * median, "mono": False}
            blocks.append({
                "id": "p%02d-b%02d" % (p["index"] + 1, len([x for x in blocks if x["page"] == p["index"] + 1]) + 1),
                "page": p["index"] + 1, "bbox": [round(v, 2) for v in r], "align": align,
                "pitch": round(pitch, 3) if pitch else None, "base": base,
                "styles": {"s1": dict(base, bold=True)}, "text": text, "pictures": [], "math": math,
                "redact": [[round(v, 2) for v in l] for l in lines],
            })
        pix = None
    os.makedirs(work, exist_ok=True)
    json.dump(blocks, open(os.path.join(work, "blocks.json"), "w"), ensure_ascii=False, indent=1)
    print(f"{len(doc)} pages, {len(blocks)} blocks, {len({b['text'] for b in blocks})} unique texts, "
          f"{skipped} boxes left as they are (figure labels, unmatched OCR) -> {work}/blocks.json")


def block_html(block, zh, bg):
    """slides.block_html with math tokens set as their HTML."""
    zh = re.sub(r"⟦E(\d+)⟧", lambda m: block["math"].get(m.group(1), ""), zh)
    return slides.block_html(dict(block, pictures=[]), zh, bg)


def render(deck, work, out):
    blocks = json.load(open(os.path.join(work, "blocks.json")))
    zh = json.load(open(os.path.join(work, "zh.json")))
    todo = [b for b in blocks if zh.get(b["text"]) and zh[b["text"]].strip() != b["text"].strip()]
    doc = pymupdf.open(deck)
    arch = pymupdf.Archive(slides.deck_fonts(work, [zh[b["text"]] for b in todo] +
                                             [v for b in todo for v in b["math"].values()]))
    recolored = slides.recolor_images(doc, set())
    report = []
    for pno, page in enumerate(doc):
        for b in [b for b in todo if b["page"] == pno + 1]:
            r = pymupdf.Rect(b["bbox"])
            bg = slides.background(page, r + (-2, -2, 2, 2))
            pad = 0.3 * b["base"]["size"]  # OCR line boxes hug the glyphs; italics and descenders reach past them
            for l in b["redact"]:
                page.draw_rect(pymupdf.Rect(l) + (-pad, -2, pad, 2), color=None, fill=bg, overlay=True)
            grow = b["base"]["size"] * 0.6
            box = pymupdf.Rect(r.x0, r.y0, r.x1 + grow, r.y1 + grow) & page.rect
            spare, scale = page.insert_htmlbox(box, block_html(b, zh[b["text"]], bg), css=slides.CSS, archive=arch,
                                               scale_low=0)
            if spare < 0 or scale < 0.999:
                report.append(f"{b['id']} shrunk to {scale:.2f}")
    for pno in reversed([i for i, p in enumerate(pymupdf.open(deck)) if slides.is_ocw_notice(p)]):
        doc.delete_page(pno)
    doc.subset_fonts()
    doc.save(out, garbage=4, deflate=True)
    print(f"wrote {out}; {recolored} page pictures recoloured, {len(todo)} blocks replaced")
    for line in report:
        print("  ", line)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "extract" and len(sys.argv) == 5:
        extract(sys.argv[2], sys.argv[3], sys.argv[4])
    elif cmd == "render" and len(sys.argv) == 5:
        render(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit("usage: ocr_slides.py extract <deck.pdf> <boxes.json> <work> | render <deck.pdf> <work> <out.pdf>")
