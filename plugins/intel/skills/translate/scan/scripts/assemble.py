#!/usr/bin/env python3
# ABOUTME: Turns the per-page OCR Markdown of a scanned document into a translate-skill work dir: figures cropped
# ABOUTME: from the page images, pages joined where a paragraph runs on, sections split, sections.json written.
#
# Usage: assemble.py <work> --source <original.pdf> [--author A] [--title T] [--one-section] [--ink]
# Reads <work>/proof/NN.md (else <work>/ocr/NN.md) and <work>/pages/p-NN.png; writes <work>/source/NN-slug.md,
# <work>/images/*.png + images.json and <work>/sections.json (with the byline, for the title cover). A references
# section is written straight to <work>/translated/ untranslated, since citations stay in English.
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

from PIL import Image

# The typeset-paper pipeline's rules, shared so both kinds of paper come out alike: publisher boilerplate to drop
# (the ACM permission footnote, the copyright line) and printed URLs as short Markdown links.
# Also the site boilerplate page some course PDFs end with.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "paper" / "scripts"))
from paper_rules import OCW_PAGE_RE, PUBLISHER_RE, link_label, linkify  # noqa: E402

MD_LINK_RE = re.compile(r"\[([^\]]+)\]\((https?://[^)\s]+)\)")


def links(text):
    """Printed URLs as short links; a link the OCR already wrote with its own address as the label gets the host
    name instead."""
    return MD_LINK_RE.sub(lambda m: f"[{link_label(m.group(1), m.group(2))}]({m.group(2)})", linkify(text))

VISION = Path(__file__).resolve().parent / "vision.mjs"
PAGE_SIZE = [427.92, 660]  # the page the translated papers are set on, as in translate-paper
OUR_COLUMN = PAGE_SIZE[0] * (1 - 2 * 0.135)  # its text column, inside the 13.5% side margins
FIGURE_RE = re.compile(r"^\[FIGURE ([\d.]+),\s*([\d.]+),\s*([\d.]+),\s*([\d.]+)\]\s*$")
NUMBERED_RE = re.compile(r"^## (\d+|[A-Z])(\.\d+)*\.?\s")
KEEP_LEVEL_RE = re.compile(r"^## (Abstract|References|Bibliography|Acknowledge?ments?|Appendix\b)", re.I)
REFS_RE = re.compile(r"^(\d+\.?\s+)?(References|Bibliography)\s*$", re.I)


def page_markdown(work):
    """[(page number, Markdown)] in page order, proofread text where it exists."""
    out = []
    for png in sorted((work / "pages").glob("p-*.png")):
        n = png.stem.split("-")[1].zfill(2)
        md = work / "proof" / f"{n}.md"
        out.append((n, png, (md if md.exists() else work / "ocr" / f"{n}.md").read_text()))
    return out


def vision_lines(png):
    """[(x0, y0, x1, y1, text)] for each text line macOS Vision finds on the page, in page fractions."""
    out = subprocess.run(["node", str(VISION), str(png)], capture_output=True, text=True, check=True).stdout
    return [(*map(float, box.split()), text) for box, text in (l.split("\t", 1) for l in out.splitlines() if "\t" in l)]


def words_of(text):
    return re.findall(r"[a-z]{3,}", text.lower())


def neighbour_text(md_lines, at, reach=2):
    """The words of the text lines next to a [FIGURE] line (up to `reach` on each side), skipping captions and
    other figures: the text a loose figure box is likely to take in."""
    def side(step):
        out, i = [], at + step
        while 0 <= i < len(md_lines) and len(out) < reach:
            t = md_lines[i].strip()
            if t and not FIGURE_RE.match(t) and not re.match(r"\*\*(Figure|Table)", t):
                out.append(t)
            i += step
        return out
    return " ".join(words_of(" ".join(side(-1)[::-1] + side(1))))


def figure_box(im, box, lines, neighbours, step=0.006):
    """The pixel box to crop for a figure the model boxed at `box` (page fractions). The model's boxes vary from
    run to run, sometimes taking in the text line under a sketch, sometimes cutting off a label at its side, so
    the box is (1) walled off from the transcribed text right before and after the figure: a Vision line of four
    or more words found in that order in `neighbours` bounds it from above or below; (2) grown on each side while
    ink touches it, a `step` of the page at a time, which brings in labels and strokes the box cut; (3) snapped
    to the ink inside, plus a small margin."""
    W, H = im.size
    ink = im.convert("L").point(lambda v: 255 if v < 170 else 0)
    top, bottom = 0.0, 1.0
    for lx0, ly0, lx1, ly1, text in lines:
        ws = words_of(text)
        if len(ws) < 4 or " ".join(ws) not in neighbours or lx0 > box[2] or lx1 < box[0]:
            continue
        margin = (ly1 - ly0) * 0.15  # Vision's line box clips ascenders and descenders
        if (ly0 + ly1) / 2 < (box[1] + box[3]) / 2:
            top = max(top, ly1 + margin)
        else:
            bottom = min(bottom, ly0 - margin)
    x0, y0, x1, y1 = box[0], max(box[1], top), box[2], min(box[3], bottom)

    def has_ink(a, b, c, d):
        return a < c and b < d and ink.crop((int(a * W), int(b * H), int(c * W), int(d * H))).getbbox() is not None

    for _ in range(400):
        grown = False
        if x0 > 0 and has_ink(max(0, x0 - step), y0, x0, y1):
            x0, grown = max(0.0, x0 - step), True
        if x1 < 1 and has_ink(x1, y0, min(1, x1 + step), y1):
            x1, grown = min(1.0, x1 + step), True
        if y0 > top and has_ink(x0, max(top, y0 - step), x1, y0):
            y0, grown = max(top, y0 - step), True
        if y1 < bottom and has_ink(x0, y1, x1, min(bottom, y1 + step)):
            y1, grown = min(bottom, y1 + step), True
        if not grown:
            break
    region = (int(x0 * W), int(y0 * H), int(x1 * W), int(y1 * H))
    bb = ink.crop(region).getbbox()
    if not bb:
        return region
    pad = round(W * 0.01)
    return (max(0, region[0] + bb[0] - pad), max(int(top * H), region[1] + bb[1] - pad),
            min(W, region[0] + bb[2] + pad), min(int(bottom * H), region[1] + bb[3] + pad))


def ink_only(im):
    """Handwritten notes on graph paper: every ink colour as black on white, the paper and its pale grid dropped,
    so the render treats the sketch as line art and recolours it to the page's ink."""
    lum = im.convert("L")
    return lum.point(lambda v: 255 - max(0, min(255, (200 - v) * 255 // 120)))


def place_figures(work, n, png, md, images, ink=False):
    """Crops each [FIGURE] box out of the page image and puts its ⟦IMG:key⟧ token in place."""
    lines, k = [], 0
    im = vision = None
    md_lines = md.splitlines()
    for at, line in enumerate(md_lines):
        m = FIGURE_RE.match(line.strip())
        if not m:
            lines.append(line)
            continue
        im = im or Image.open(png)
        vision = vision if vision is not None else vision_lines(png)
        k += 1
        key = f"p{n}-f{k}"
        crop = im.crop(figure_box(im, [float(v) for v in m.groups()], vision, neighbour_text(md_lines, at)))
        (ink_only(crop) if ink else crop).save(work / "images" / f"{key}.png")
        images[key] = {"file": f"{key}.png", "block": True}
        lines.append(f"⟦IMG:{key}⟧")
    return "\n".join(lines)


def is_body_paragraph(block):
    return not re.match(r"(#|\[FIGURE|⟦IMG|\*\*(Figure|Table)|Note:|\||```|[-*] |\\\[)", block)


LIST_RE = re.compile(r"^\s*([-*+]|\d+[.)])\s")


def blockify(md):
    """A page's Markdown as blocks. The OCR writes one paragraph per line, sometimes without blank lines, so each
    line is a block, except that a table, a code fence, a display equation, a list and lines joined by a hard
    break ("  " at the end) stay whole."""
    blocks, cur, fence, math = [], [], False, False

    def close():
        if cur:
            blocks.append("\n".join(cur).strip())
            cur.clear()

    for line in md.splitlines():
        s = line.strip()
        if fence or math:
            cur.append(line)
            fence = fence and not s.startswith("```")
            math = math and not s.endswith("\\]")
            continue
        if not s:
            close()
            continue
        prev = cur[-1] if cur else ""
        same = (prev.endswith("  ") or (s.startswith("|") and prev.lstrip().startswith("|"))
                or (LIST_RE.match(line) or line.startswith("  ")) and LIST_RE.match(cur[0] if cur else ""))
        if not same:
            close()
        cur.append(line)
        fence = s.startswith("```") and not (len(s) > 3 and s.endswith("```"))
        math = s.startswith("\\[") and not s.endswith("\\]")
    close()
    return [b for b in blocks if b]


def join_pages(pages):
    """Concatenates the pages' blocks; a page's <<CONT>> block continues the last body paragraph of the pages
    before it (not a table or legend placed above it on its own page), rejoining a word hyphenated across the
    page break. A title on a later page starts a section inside the document."""
    blocks = []
    for md in pages:
        if OCW_PAGE_RE.search(md):
            continue
        page_start = len(blocks)
        parts = blockify(md)
        cont = False
        for b in parts:
            if b == "<<CONT>>":
                cont = True
                continue
            if b.startswith("<<CONT>>"):
                cont, b = True, b[len("<<CONT>>"):].strip()
            if cont and is_body_paragraph(b):
                cont = False
                at = next((i for i in range(page_start - 1, -1, -1) if is_body_paragraph(blocks[i])), None)
                if at is not None:
                    prev = blocks[at]
                    blocks[at] = prev[:-1] + b if re.search(r"[a-z]-$", prev) and b[:1].islower() else f"{prev} {b}"
                    continue
            if page_start and b.startswith("# "):
                b = "#" + b
            if not PUBLISHER_RE.match(b):
                blocks.append(b if b.startswith(("```", "|")) else links(b))
    return blocks


def demote_unnumbered(blocks):
    """In a paper whose sections are numbered, an unnumbered '## ' heading is a sub-section."""
    if not any(NUMBERED_RE.match(b) for b in blocks):
        return blocks
    return ["#" + b if b.startswith("## ") and not NUMBERED_RE.match(b) and not KEEP_LEVEL_RE.match(b) else b
            for b in blocks]


def take_byline(blocks):
    """Splits off the short blocks that open the first section (authors, affiliations), one line each: they go on
    the cover under the English title, not into the translated body."""
    lines, at = [], 0
    while at < len(blocks) and not blocks[at].startswith("#") and len(blocks[at].split()) < 40 and is_body_paragraph(blocks[at]):
        lines.append(re.sub(r"\s*\n\s*", " ", blocks[at].strip()))
        at += 1
    return lines, blocks[at:]


def notes_last(body):
    """A section's footnotes ("Note: " blocks) moved after a closing "---" rule, where the paper renderer sets
    them small."""
    notes = [b[len("Note:"):].strip() for b in body if b.startswith("Note:")]
    rest = [b for b in body if not b.startswith("Note:")]
    return rest + ["---"] + notes if notes else rest


def split_sections(blocks):
    """[(title, body blocks)]: the title block, byline and abstract are the first section, named after the
    document; then one section per '## ' heading other than the abstract."""
    title = next((b[2:].strip() for b in blocks if b.startswith("# ")), "")
    sections = [[title, []]]
    for b in blocks:
        if b.startswith("# "):
            continue
        if b.startswith("## ") and not b.lower().startswith("## abstract"):
            sections.append([b[3:].strip(), []])
        else:
            sections[-1][1].append(re.sub(r"^### ", "## ", b))
    return [(t, body) for t, body in sections if body]


def column_px(pages):
    """The source's text column width in page-image pixels: the median width of the lines of six or more words
    macOS Vision finds on the pages."""
    widths = []
    for _, png, _ in pages:
        W = Image.open(png).size[0]
        widths += [(x1 - x0) * W for x0, _, x1, _, text in vision_lines(png) if len(text.split()) >= 6]
    widths.sort()
    return widths[len(widths) // 2] if widths else None


def slug(text):
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:40] or "section"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("work")
    ap.add_argument("--source", help="the original PDF; translate-paper picks a course glossary from its folder")
    ap.add_argument("--author", default="")
    ap.add_argument("--title", help="the document title, for documents without a '# ' title line (lecture notes)")
    ap.add_argument("--one-section", action="store_true", help="keep the whole document as one section (lecture notes)")
    ap.add_argument("--ink", action="store_true", help="handwritten on graph paper: figures as black ink on white, grid dropped")
    opt = ap.parse_args()
    work = Path(opt.work).resolve()
    for d in ("images", "source", "translated"):
        (work / d).mkdir(exist_ok=True)
    images = {}
    pages = page_markdown(work)
    texts = [place_figures(work, n, png, md, images, opt.ink) for n, png, md in pages]
    blocks = [re.sub(r"^(#+ )\*\*(.*)\*\*\s*$", r"\1\2", b) for b in join_pages(texts)]  # no bold in headings
    if opt.one_section:
        sections = [(opt.title, [b for b in blocks if not b.startswith("# ")])]
    else:
        sections = split_sections(demote_unnumbered(blocks))
        if opt.title:
            sections[0] = (opt.title, sections[0][1])
    byline = []
    if not opt.one_section:
        byline, rest = take_byline(sections[0][1])
        sections[0] = (sections[0][0], rest)
    col = column_px(pages)

    meta = {"title": sections[0][0], "author": opt.author, "byline": byline, "page_size": PAGE_SIZE,
            "source_kind": "scan", "source_pdf": str(Path(opt.source).resolve()) if opt.source else None, "sections": [],
            # figure crops display at their source size, scaled from the source's text column to ours
            "img_scale": round(OUR_COLUMN / col, 4) if col else None}
    for i, (title, body) in enumerate(sections):
        body = notes_last(body)
        sid = f"{i:02d}"
        file = f"source/{sid}-{slug(title)}.md"
        text = f"# {title}\n\n" + "\n\n".join(body) + "\n"
        (work / file).write_text(text)
        meta["sections"].append({"id": sid, "kind": "chapter", "title": title, "file": file,
                                 "words": len(re.findall(r"[A-Za-z]+", text))})
        if REFS_RE.match(title):
            (work / "translated" / Path(file).name).write_text("# 参考文献\n\n" + "\n\n".join(body) + "\n")
        print(f"{sid} {title}: {meta['sections'][-1]['words']} words")
    (work / "sections.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2))
    (work / "images.json").write_text(json.dumps(images, indent=2))
    print(f"{len(images)} figures")


if __name__ == "__main__":
    main()
