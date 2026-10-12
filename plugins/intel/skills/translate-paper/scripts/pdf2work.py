#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pymupdf>=1.24"]
# ///
# ABOUTME: Turns a typeset PDF (paper, lecture notes) into a translate work dir: MinerU parses the layout, then the
# ABOUTME: typed blocks become per-section Markdown with LaTeX math, cropped figures, code, footnotes and references.
#
# Usage: pdf2work.py <paper.pdf> <work dir> [--mineru <mineru-kit>] [--single]
# Writes <work>/sections.json, src/<id>.md (to translate), src/<id>.ref.txt (the PDF text layer of the same blocks,
# for checking symbols), translated/<id>.md for references (kept verbatim), images/ + images.json, codeblocks.json.
# Re-running reuses <work>/mineru/ (delete it to re-parse). MinerU is $MINERU_KIT, else the one setup.sh installs.
import argparse
import collections
import json
import os
import re
import statistics
import subprocess
import sys
import zipfile
from html.parser import HTMLParser
from pathlib import Path

import pymupdf

sys.path.insert(0, str(Path(__file__).resolve().parent))
from paper_rules import OCW_PAGE_RE, drop_publisher, link_label, linkify  # noqa: E402

PAGE_SIZE = [427.92, 660]  # the house page size of the translated books (pt)
SIDE_MARGIN = 0.135  # render.py's side margin, as a share of the page width
CROP_ZOOM = 2  # figure crops are rendered at 2 px per pt (144 dpi), the crop box only, never the whole page
DROP = {"header", "footer", "page_number", "page_header", "page_footer", "aside_text"}
BACK_TITLES = re.compile(r"^(\d+\.?\s*)?(references|bibliography)$", re.I)
NUMBERED = re.compile(r"^(\d+)\.?\s+\S")  # "1. INTRODUCTION", "2 Background"
SUBNUMBERED = re.compile(r"^\d+\.\d+")
BULLET = re.compile(r"^\s*[•●◦▪∙·\-–—]\s*")
BULLET_CHAR = re.compile(r"^\s*[•●◦▪∙]\s*")
# A line of a printed table of contents ("8.1 Faults ......... 8–2"): dropped, the book format builds its own 目录.
DOT_LEADER = re.compile(r"(\.\s?){3,}[\s.]*\d+([–-]\d+)?\**\s*$|\s\.+\s*\d+[–-]\d+\**\s*$")
# Running page labels and chapter-opener fragments: "8–1", "CHAPTER", a bare chapter number.
PAGE_LABEL = re.compile(r"^\**(\d+[–-]\d+|chapter|\d+)\**$", re.I)
CONTENTS_TITLE = re.compile(r"^(chapter\s+)?(table of\s+)?contents$", re.I)
SPLIT_WORDS = 2500  # documents shorter than this stay one section
MIN_SECTION_WORDS = 120  # a shorter top-level section (acknowledgments) is merged into the previous one
MAX_SECTION_WORDS = 4000  # a longer section is split at its headings: one translation call per piece
DEFAULT_MINERU = Path.home() / ".local/share/intel/translate-paper/mineru/bin/mineru-kit"


def run_mineru(pdf, work, mineru):
    out = work / "mineru"
    middle = out / "x" / "middle_json.json"
    if middle.exists():
        return json.loads(middle.read_text())
    out.mkdir(parents=True, exist_ok=True)
    subprocess.run([mineru, "parse", str(pdf), "-o", str(out), "-f", "zip", "--tier", "advanced",
                    "--disable-image-analysis"], check=True, stdout=subprocess.DEVNULL, stderr=(out / "mineru.log").open("w"))
    zips = list(out.glob("*.zip"))
    if not zips:
        sys.exit(f"mineru wrote no zip; see {out / 'mineru.log'}")
    with zipfile.ZipFile(zips[0]) as z:
        z.extractall(out / "x")
    return json.loads(middle.read_text())


TEXT_MODE_CMDS = {"text", "textbf", "textit", "textrm", "mathrm", "emph", "quad", "qquad", "ldots", "dots", "%", "&", "_", "#"}


def group_end(tex, start):
    """The index of the brace closing the group that opens at tex[start]; escaped braces (\\{ \\}) do not count."""
    depth, i = 0, start
    while i < len(tex):
        if tex[i] == "\\":
            i += 2
            continue
        depth += {"{": 1, "}": -1}.get(tex[i], 0)
        if depth == 0:
            return i
        i += 1
    return len(tex) - 1


def math_in_text(tex):
    """Lifts math commands MinerU left inside \\text{...} out of it: \\text{lemma A(t) = \\hat{A} (t)} ->
    \\text{lemma A(t) = }\\hat{A}\\text{ (t)}. Bare they are a parse error ("Can't use function '\\hat' in text mode");
    $...$ would parse too, but render.py escapes every $ in math as a currency sign."""
    out, i = [], 0
    while (j := tex.find("\\text", i)) >= 0:
        k = j + 5
        while k < len(tex) and tex[k] == " ":
            k += 1
        if k >= len(tex) or tex[k] != "{" or (k == j + 5 and tex[k - 1:k].isalpha()):
            out.append(tex[i:k]); i = k
            continue
        e = group_end(tex, k)
        body = tex[k + 1:e]
        fixed, n = [], 0
        while n < len(body):
            sub = re.match(r"[A-Za-z0-9]+(?:[_^](?:\{[^{}]*\}|[A-Za-z0-9]))+", body[n:])
            if sub and (n == 0 or not body[n - 1].isalnum()):  # a sub- or superscript: "if T_{k}" -> "if $T_{k}$"
                fixed.append("}" + sub.group(0) + "\\text{")
                n += sub.end()
                continue
            m = re.match(r"\\([a-zA-Z]+)", body[n:])
            if m and m.group(1) not in TEXT_MODE_CMDS:
                end = n + m.end()
                while True:  # take the command's brace argument(s), with any spaces before each
                    nxt = len(body[end:]) - len(body[end:].lstrip(" "))
                    if end + nxt >= len(body) or body[end + nxt] != "{":
                        break
                    end += nxt
                    end = group_end(body, end) + 1
                fixed.append("}" + body[n:end] + "\\text{")
                n = end
            else:
                fixed.append(body[n]); n += 1
        out.append(tex[i:k + 1] + "".join(fixed) + "}")
        i = e + 1
    out.append(tex[i:])
    return "".join(out)


# Text-mode symbol commands MinerU emits inside math, which KaTeX does not know, and their math equivalents.
TEXT_SYMBOLS = {"textgreater": ">", "textless": "<", "textellipsis": r"\ldots ", "textbullet": r"\bullet ",
                "textasciitilde": r"\sim ", "textbar": "|", "textbackslash": r"\backslash ", "textendash": "-",
                "textemdash": "-", "textmu": r"\mu "}


def tidy_math(tex):
    """MinerU's LaTeX: collapse whitespace runs and strip a trailing sentence punctuation, returned separately."""
    tex = re.sub(r"\s+", " ", tex).strip()
    # MinerU's formula model sometimes loops, repeating one short piece hundreds of times ("{ } { } { } ..."): kept once
    tex = re.sub(r"(\{ ?\} ?){2,}", "", tex)
    tex = re.sub(r"(.{1,24}?)\1{9,}", r"\1", tex)
    # an unclosed group MinerU cut short ("\mathbf { \hat { A d v } _ { 3 }": closed at the end
    tex += "}" * max(0, len(re.findall(r"(?<!\\)\{", tex)) - len(re.findall(r"(?<!\\)\}", tex)))
    tex = re.sub(r"(?<!\\)\$", "", tex)  # MinerU's stray inner delimiters: "(i.e., $\mathtt{o} = ...$)" inside a formula
    tex = math_in_text(tex)
    # text-mode comparison commands MinerU emits, which KaTeX does not know
    for cmd, sub in TEXT_SYMBOLS.items():
        tex = re.sub(r"\\" + cmd + r"(?![a-zA-Z])", lambda _: sub, tex)
    tex = re.sub(r"\s+", " ", tex)
    tex = re.sub(r"\(?\\(ref|eqref|label)\s*\{[^{}]*\}\)?", "", tex).strip()  # cross-reference commands KaTeX lacks
    # a hyphenated name set upright (\mathrm{ORD-MULT}) would typeset its hyphen as a minus sign
    tex = re.sub(r"\\mathrm\s*\{([A-Za-z ]+-[A-Za-z -]+)\}", lambda m: "\\text{" + m.group(1).replace(" ", "") + "}", tex)
    m = re.search(r"\s*([,.;:])\s*$", tex)
    tail = ""
    # "\right." (an invisible delimiter) and "\," (a thin space) end in punctuation that belongs to the formula
    if m and not tex.endswith("\\" + m.group(1)) and not re.search(r"\\(right|[bB]igg?r)\s*\.$", tex):
        tail, tex = m.group(1), tex[:m.start()].rstrip()
    return tex, tail


SUPERSCRIPT_DIGITS = str.maketrans("0123456789", "⁰¹²³⁴⁵⁶⁷⁸⁹")
# The numbers that open the document's footnotes; a superscript number elsewhere is an exponent. Set by convert().
FOOTNOTE_MARKS = set()


def footnote_marks(middle, is_note=lambda page_idx, bbox: False):
    """The leading numbers of the document's footnotes: its page_footnote blocks, and the text blocks `is_note`
    takes for footnotes (MinerU types some as plain text)."""
    marks = set()
    for p in middle["pages"]:
        for b in p["blocks"]:
            note = b["type"] == "page_footnote" or b["type"] == "text" and is_note(p["page_idx"], b["bbox"])
            if note and isinstance(b.get("content"), list):
                # a block can hold several footnotes ("5 As mentioned ... 6 This approach ..."): every raised number
                marks.update(c["content"].strip() for c in b["content"] if isinstance(c.get("content"), str)
                             and "superscript" in c.get("styles", []) and c["content"].strip().isdigit())
                first = "".join(str(s.get("content", "")) for s in b["content"][:2] if isinstance(s.get("content"), str))
                m = re.match(r"\s*\^?\s*\{?\s*(\d+)", first)
                if m:
                    marks.add(m.group(1))
    return marks


def prose_as_math(tex):
    """The text of a formula that is really a phrase (spaced letters forming words, an e.g. or i.e., \\; for word
    spaces), with a trailing raised digit as a footnote mark or superscript; None for real math."""
    t = re.sub(r"\\(it|tt|rm|bf|sf|mathrm|mathit|mathtt|texttt|text)\b|[{}]", "", tex)
    t = re.sub(r"\\[;:, ]|\\quad", "\x00", t)
    mark = ""
    if m := re.search(r"\^\s*(\d+)\s*(\)?)\s*$", t):
        mark, t = m.group(1), t[:m.start()] + m.group(2)
    t = re.sub(r"\s+", "", t).replace("\x00", " ").strip()
    if not re.fullmatch(r"[()A-Za-z0-9.,;:'’ -]+", t) or not re.search(r"[A-Za-z]{3,}|\b[a-z]\.[a-z]\.", t):
        return None
    if mark:
        sup = f"<sup>{mark}</sup>" if mark in FOOTNOTE_MARKS else mark.translate(SUPERSCRIPT_DIGITS)
        t = t[:-1] + sup + ")" if t.endswith(")") else t + sup
    return t


def note_mark(text):
    """A footnote whose number the PDF prints unraised ("3http://...", "2 To avoid ...") gets it as a <sup> mark, which the
    translation check then requires, so the footnote cannot be dropped."""
    return re.sub(r"^(\d{1,2})\s?(?=[A-Z\[(])", r"<sup>\1</sup>", text)


def spans_md(spans):
    """Inline spans (text, equation_inline, hyperlink) -> Markdown with \\(..\\) math and <sup> footnote marks."""
    out = []
    for s in spans:
        t, c = s.get("type"), s.get("content", "")
        styles = s.get("styles", [])
        if t == "equation_inline":
            tex, tail = tidy_math(c)
            if (m := re.fullmatch(r"\^\s*\{?\s*(\d+)\s*\}?", tex)) and re.search(r"(^|[^A-Za-z])[A-Z] ?$", "".join(out)):
                if out[-1].endswith(" "):  # a name with a raised digit set as math: "L ^{4} Linux" -> "L⁴Linux"
                    out[-1] = out[-1][:-1]
                out.append(m.group(1).translate(SUPERSCRIPT_DIGITS) + tail)
            elif re.fullmatch(r"\^\s*\{?\s*([1-9]\d*|[*†‡§¶])\s*\}?", tex):  # a footnote mark set as math: ^{1}
                out.append(f"<sup>{re.sub(r'[^0-9*†‡§¶]', '', tex)}</sup>{tail}")
            elif re.fullmatch(r"\^\s*\{?\s*\\(dagger|ddagger)\s*\}?", tex):
                out.append(("†" if "ddagger" not in tex else "‡") + tail)
            elif m := re.fullmatch(r"\^\s*\{?\s*(th|st|nd|rd)\s*\}?", tex):  # an ordinal suffix: 99th
                out.append(m.group(1) + tail)
            elif m := re.fullmatch(r"([\d\s.]*(?:\\dot\s*\{\s*\d\s*\}[\d\s]*)?)\^\s*\{\s*(t\s*h|s\s*t|n\s*d|r\s*d)\s*\}", tex):
                # a number with its ordinal suffix set as math: "9 9 . \dot { 9 } ^ { t h }" -> 99.9th
                num = re.sub(r"\\dot\s*\{\s*(\d)\s*\}", r"\1", m.group(1)).replace(" ", "")
                out.append(num + m.group(2).replace(" ", "") + tail)
            elif m := re.fullmatch(r"\^\s*\{?\s*\\circ\s*\}?\s*(\w)\s*\^\s*\{?\s*\\(prime|circ)\s*\}?", tex):
                out.append(f"'{m.group(1)}'" + tail)  # a quoted character read as degrees and primes: '0'
            elif prose := prose_as_math(tex):  # a phrase set as math: "( { \it e . g . , \; \tt b a d . c o m } )"
                out.append(prose + tail)
            elif m := re.fullmatch(r"\\S\s*\{?\s*([\d.]+)\s*\}?", tex):  # a section reference: §4
                out.append("§" + m.group(1).replace(" ", "") + tail)
            elif re.fullmatch(r"\d+(\.\d+)?\s*\\%", tex):  # a percentage: 5%
                out.append(tex.replace("\\%", "%").replace(" ", "") + tail)
            elif re.fullmatch(r"\^\s*\{?\s*(0|\\prime)\s*\}?", tex):  # a lone prime mark
                out.append("′" + tail)
            elif re.fullmatch(r"\[\s*\d+(\s*[,–-]\s*\d+)*\s*\]", tex):  # a citation read as math: [5, 20]
                out.append(re.sub(r"\s+", "", tex).replace(",", ", ") + tail)
            elif tex:
                out.append(f"\\({tex}\\){tail}")
        elif t == "hyperlink":
            label = "".join(str(x.get("content", "")) for x in c) if isinstance(c, list) else str(c)
            url = s.get("url") or label
            out.append(f"[{link_label(label, url)}]({url})")
        else:
            c = re.sub(r"[\x00-\x08\x0b-\x1f]", "", c.replace("\t", " "))  # a bullet glyph can arrive as NUL
            if "superscript" in styles and re.fullmatch(r"0+", c.strip()):
                c = "′" * len(c.strip())  # TeX's prime glyph is a superscript "0" in the text layer: k′, not k<sup>0</sup>
            elif "superscript" in styles and c.strip().isdigit() and (c.strip() not in FOOTNOTE_MARKS
                                                                     or re.search(r"(^|[^A-Za-z])[A-Z] ?$", "".join(out))):
                # an exponent, not a footnote mark: n², and a name like L⁴ (a raised digit right after a lone capital)
                c = c.strip().translate(SUPERSCRIPT_DIGITS)
                if out and out[-1].endswith(" ") and re.search(r"[A-Z] $", out[-1]):
                    out[-1] = out[-1][:-1]  # "L ⁴" -> "L⁴"
            elif "superscript" in styles and c.strip():
                c = f"<sup>{c.strip()}</sup>"
            elif "subscript" in styles and c.strip():
                c = f"<sub>{c.strip()}</sub>"
            elif "bold" in styles and c.strip():
                c = f"**{c.strip()}**"
            out.append(c)
    text = linkify("".join(out))
    text = re.sub(r"(?<=[\])]) +(?=[,.;:](\s|$))", "", text)  # "[5, 20] , which" -> "[5, 20], which"
    return re.sub(r"[ \u00a0]{2,}", " ", text).strip()


class TableParser(HTMLParser):
    """MinerU's table_body HTML -> rows of cell strings, noting any row/col span."""

    def __init__(self):
        super().__init__()
        self.rows, self.cell, self.spans = [], None, False

    def handle_starttag(self, tag, attrs):
        if tag == "tr":
            self.rows.append([])
        elif tag in ("td", "th"):
            self.cell = []
            self.spans |= any(k in ("rowspan", "colspan") and v != "1" for k, v in attrs)

    def handle_endtag(self, tag):
        if tag in ("td", "th") and self.cell is not None:
            self.rows[-1].append(" ".join("".join(self.cell).split()))
            self.cell = None

    def handle_data(self, data):
        if self.cell is not None:
            self.cell.append(data)


def table_md(body_html):
    """A pipe table for a plain grid; None when the table has merged cells (it is cropped as an image instead)."""
    p = TableParser()
    p.feed(body_html)
    if p.spans or not p.rows or len({len(r) for r in p.rows}) != 1:
        return None
    esc = lambda c: re.sub(r"\$([^$]+)\$", r"\\(\1\\)", c).replace("|", "\\|")
    lines = ["| " + " | ".join(esc(c) for c in p.rows[0]) + " |", "|" + "---|" * len(p.rows[0])]
    lines += ["| " + " | ".join(esc(c) for c in r) + " |" for r in p.rows[1:]]
    return "\n".join(lines)


def words(md):
    """English words in a Markdown text, not counting math, tokens, tags or table rows (the translator's length
    floor: a table of identifiers such as assembler macros has words that stay English in the translation)."""
    md = re.sub(r"^\|.*$", " ", md, flags=re.M)
    md = re.sub(r"\\\(.*?\\\)|\\\[.*?\\\]|⟦[^⟧]*⟧|<[^>]+>", " ", md, flags=re.S)
    return len(re.findall(r"[A-Za-z]{2,}", md))


class Doc:
    def __init__(self, pdf, work, middle):
        self.pdf, self.work, self.middle = pdf, work, middle
        self.fz = pymupdf.open(pdf)
        self.images, self.codes = {}, {}
        (work / "images").mkdir(parents=True, exist_ok=True)
        self.body_size = self._body_font_size()
        self.text_col = self._text_column_width()

    def rect(self, page_idx, bbox, pad=0.0):
        r = self.fz[page_idx].rect
        x0, y0, x1, y1 = bbox
        return pymupdf.Rect((x0 - pad) * r.width, (y0 - pad) * r.height, (x1 + pad) * r.width, (y1 + pad) * r.height) & r

    def layer_text(self, page_idx, bbox):
        return self.fz[page_idx].get_text("text", clip=self.rect(page_idx, bbox, 0.002)).strip()

    def font_size(self, page_idx, bbox):
        """The character-weighted median font size inside a block, from the PDF text layer."""
        sizes = []
        for b in self.fz[page_idx].get_text("dict", clip=self.rect(page_idx, bbox))["blocks"]:
            for line in b.get("lines", []):
                for s in line["spans"]:
                    sizes += [round(s["size"], 1)] * len(s["text"].strip())
        return statistics.median(sizes) if sizes else None

    def has_bold(self, page_idx, bbox):
        """True when the PDF sets any of the block's text in a bold face (MinerU marks italic text bold too)."""
        for b in self.fz[page_idx].get_text("dict", clip=self.rect(page_idx, bbox))["blocks"]:
            for line in b.get("lines", []):
                for s in line["spans"]:
                    if s["text"].strip() and ("bold" in s["font"].lower() or s["flags"] & 16):
                        return True
        return False

    def is_note(self, page_idx, bbox):
        """A block set smaller than the body text in the lower part of a page: a footnote, whatever MinerU typed it."""
        size = self.font_size(page_idx, bbox)
        return bool(size) and size < 0.9 * self.body_size and bbox[1] > 0.55

    def _body_font_size(self):
        c = collections.Counter()
        for p in self.middle["pages"]:
            for b in p["blocks"]:
                if b["type"] == "text":
                    s = self.font_size(p["page_idx"], b["bbox"])
                    if s:
                        c[s] += 1
        return c.most_common(1)[0][0] if c else 10

    def _text_column_width(self):
        """Median width (pt) of the body text blocks: one column's width in a two-column paper."""
        ws = [(b["bbox"][2] - b["bbox"][0]) * self.fz[p["page_idx"]].rect.width
              for p in self.middle["pages"] for b in p["blocks"] if b["type"] == "text" and words(spans_md(b["content"])) > 30]
        return statistics.median(ws) if ws else 400

    def crop(self, page_idx, bbox, key):
        """Crop a figure region from the page at CROP_ZOOM; returns its ⟦IMG⟧ token."""
        clip = self.rect(page_idx, bbox, 0.004)
        pix = self.fz[page_idx].get_pixmap(matrix=pymupdf.Matrix(CROP_ZOOM, CROP_ZOOM), clip=clip, alpha=False)
        name = f"{key}.png"
        pix.save(self.work / "images" / name)
        del pix  # one bitmap alive at a time
        self.images[key] = {"file": name, "block": True}
        return f"⟦IMG:{key}⟧"

    def code(self, text, key):
        self.codes[key] = text.rstrip("\n")
        return f"⟦CODE:{key}⟧"


CAPTION_LABEL = re.compile(r"^((?:Fig\.|Figure|Table|TABLE|Algorithm|Listing)\s*[\dIVX]+[.:]?)\s*(.*)$", re.S)


def caption_md(cap):
    """A caption with its label in bold ("**Fig. 3.** ..."), the form the renderer keeps with its figure."""
    m = CAPTION_LABEL.match(cap)
    return f"**{m.group(1)}** {m.group(2)}" if m else cap


def union(boxes):
    return [min(b[0] for b in boxes), min(b[1] for b in boxes), max(b[2] for b in boxes), max(b[3] for b in boxes)]


def overlaps(a, b):
    return a[0] < b[2] and b[0] < a[2] and a[1] < b[3] and b[1] < a[3]


def figure_groups(blocks):
    """Runs of consecutive image blocks on a page that form one figure (a grid of sub-figures, each with its own
    "(a) ..." label and only the last carrying the "Figure N" caption), when no other block lies inside their
    union. Maps the run's first block index to the run, and every other index in it to []."""
    out, i = {}, 0
    while i < len(blocks):
        j = i
        while j < len(blocks) and blocks[j]["type"] == "image":
            j += 1
        run = blocks[i:j]
        if len(run) > 1:
            box = union([b["bbox"] for b in run])
            others = [b for k, b in enumerate(blocks) if not i <= k < j and b["type"] not in DROP]
            labelled = sum(any(c["type"] == "image_caption" and CAPTION_LABEL.match(spans_md(c["content"]))
                               for c in b["content"]) for b in run)
            if labelled <= 1 and not any(overlaps(box, b["bbox"]) for b in others):
                out[i] = run
                out.update({k: [] for k in range(i + 1, j)})
        i = max(j, i + 1)
    return out


def contents_page(text):
    """A printed table of contents (the book format builds its own 目录): most of its lines are dot leaders or
    bare page numbers."""
    lines = [line for line in text.split("\n") if line.strip()]
    leaders = sum(bool(re.search(r"(\. ?){4,}|^\s*\d+\s*$", line)) for line in lines)
    return len(lines) >= 10 and leaders >= 0.5 * len(lines)


def heading_level(title, numbered_doc):
    """1 = a top-level section, 2 = a subsection. Numbered papers: "N. TITLE" and the reference/acknowledgment
    titles are top-level, "N.M" and unnumbered run-in titles are subsections. Unnumbered notes: all top-level."""
    if not numbered_doc:
        return 1
    if SUBNUMBERED.match(title):
        return 2
    if NUMBERED.match(title) or re.match(r"^(abstract|acknowledg|references|bibliography|appendix)", title, re.I):
        return 1
    return 2


def convert(pdf, work, middle):
    FOOTNOTE_MARKS.clear()
    doc = Doc(pdf, work, middle)
    FOOTNOTE_MARKS.update(footnote_marks(middle, doc.is_note))
    titles = [spans_md(b["content"]) for p in middle["pages"] for b in p["blocks"] if b["type"] == "paragraph_title"]
    numbered_doc = sum(bool(NUMBERED.match(t)) for t in titles) >= 2
    # The last title in the first three pages: a textbook chapter prints the book's title on its title page and
    # the chapter's own title two pages on.
    titles_early = [spans_md(b["content"]) for p in middle["pages"][:3] for b in p["blocks"] if b["type"] == "doc_title"]
    doc_title = titles_early[-1] if titles_early else (middle["metadata"].get("document", {}).get("title") or pdf.stem)

    # Blocks -> a flat list of items: ("h", level, text, ref) | ("p", text, ref) | ("raw", md, ref) | ("note", text, ref) | ("ref", text)
    items = []
    last_p = None  # index of the last paragraph item, for joining a paragraph continued across a column or page
    byline = []
    lead = True  # still in the byline: the short blocks between the title and the first heading or prose paragraph

    def add_p(text, ref, cont):
        nonlocal last_p
        prev = items[last_p] if last_p is not None else None
        # only floats (figures, notes) lie between them: the page or column broke the sentence around a figure,
        # so it continues whatever its case ("used by" | figure | "Internet HTTPS servers")
        interrupted = last_p is not None and len(items) - 1 > last_p
        unfinished = prev and not re.search(r"[.!?:;)\]”\"]\s*$", prev[1])
        joins = prev and (cont or unfinished and (re.match(r"^[a-z(\\]", text) or interrupted))
        if joins:
            hyphen = prev[1].endswith("-") and re.match(r"^[a-z]", text)
            joined = prev[1][:-1] + text if hyphen else prev[1] + ("" if prev[1].endswith("—") else " ") + text
            items[last_p] = ("p", joined, prev[2] + "\n" + ref)
        else:
            items.append(("p", text, ref))
            last_p = len(items) - 1

    for p in middle["pages"]:
        pi = p["page_idx"]
        margins = sorted({round(b["bbox"][0], 2) for b in p["blocks"] if b["type"] == "text"})
        if OCW_PAGE_RE.search(doc.fz[pi].get_text()) or contents_page(doc.fz[pi].get_text()):
            continue
        groups = figure_groups(p["blocks"])
        for bi, b in enumerate(p["blocks"]):
            if groups.get(bi) == []:
                continue  # a sub-figure cropped with the first block of its group
            t = b["type"]
            ref = doc.layer_text(pi, b["bbox"])
            key = f"p{pi + 1}-{b.get('index', 0)}"
            if t in DROP or t == "doc_title":
                continue
            lead = lead and t == "text" and words(spans_md(b["content"])) < 40
            if t == "paragraph_title" and (CONTENTS_TITLE.match(spans_md(b["content"])) or PAGE_LABEL.match(spans_md(b["content"]))
                                           or DOT_LEADER.search(spans_md(b["content"]))):
                continue
            if t == "paragraph_title":
                title = spans_md(b["content"])
                items.append(("h", heading_level(title, numbered_doc), title, ref))
                last_p = None
            elif t == "text":
                text = spans_md(b["content"])
                if "**" in text and not doc.has_bold(pi, b["bbox"]):
                    text = text.replace("**", "")
                text = drop_publisher(text)
                if not text or DOT_LEADER.search(text) or PAGE_LABEL.match(text):
                    continue
                if lead:  # authors and affiliations go on the cover, not into the translated body
                    byline.append(re.sub(r"\s*\n\s*", " ", text))
                    continue
                if doc.is_note(pi, b["bbox"]):
                    items.append(("note", text, ref))  # a first-page footnote MinerU typed as text
                elif BULLET_CHAR.match(text):  # a bullet item MinerU typed as a paragraph
                    items.append(("raw", "- " + BULLET_CHAR.sub("", text), ref))
                    last_p = None
                else:
                    add_p(text, ref, b.get("continues_prev"))
            elif t == "page_footnote":
                text = note_mark(drop_publisher(spans_md(b["content"])))
                if text:
                    items.append(("note", text, ref))
            elif t == "equation":
                tex, _ = tidy_math(b["content"])
                items.append(("raw", f"\\[{tex}\\]", ref))
                last_p = None  # text after a display equation ("where ...") starts its own paragraph
            elif t == "list" and doc.is_note(pi, b["bbox"]):  # first-page footnotes MinerU grouped as a list
                for it in b["content"]:
                    text = drop_publisher(spans_md(it["content"]))
                    if text:
                        items.append(("note", text, doc.layer_text(pi, it["bbox"])))
            elif t == "list":
                lines = []
                for it in b["content"]:
                    if it["type"] == "ref_text":
                        items.append(("ref", spans_md(it["content"])))
                        continue
                    text = spans_md(it["content"])
                    if DOT_LEADER.search(text):
                        continue
                    x0 = it["bbox"][0]
                    left = max([m for m in margins if m <= x0 + 0.01] or [x0])
                    indent = "  " if x0 - left > 0.025 else ""
                    if re.match(r"^(\d+|[a-z]|[ivx]+)[.)]\s|^\((\d+|[a-z]|[ivx]+)\)\s", text):
                        lines.append(f"{indent}{text}" if not indent else f"{indent}- {text}")
                    else:
                        lines.append(f"{indent}- {BULLET.sub('', text)}")
                if lines:
                    items.append(("raw", "\n".join(lines), "\n".join(doc.layer_text(pi, it["bbox"]) for it in b["content"])))
                last_p = None
            elif groups.get(bi):  # a grid of sub-figures: one crop with their "(a) ..." labels, then the caption
                group = groups[bi]
                caps = [spans_md(c["content"]) for g in group for c in g["content"] if c["type"] == "image_caption"]
                main = [c for c in caps if CAPTION_LABEL.match(c)]
                box = union([c["bbox"] for g in group for c in g["content"]
                             if c["type"] == "image_body" or c["type"] == "image_caption" and not CAPTION_LABEL.match(spans_md(c["content"]))])
                items.append(("raw", "\n\n".join([doc.crop(pi, box, f"{key}-grid")] + [caption_md(c) for c in main]), ref))
            elif t in ("image", "table", "code", "chart"):
                body = [c for c in b["content"] if c["type"].endswith("_body")]
                caps = [spans_md(c["content"]) for c in b["content"] if c["type"].endswith(("_caption", "_footnote"))]
                parts = []
                for c in body:
                    ck = f"{key}-{c.get('index', 0)}"
                    if c["type"] == "code_body":
                        parts.append(doc.code(c["content"], ck))
                    elif c["type"] == "table_body" and (md := table_md(c["content"])):
                        parts.append(md)
                    else:  # image_body, chart_body, algorithm_body, a table with merged cells
                        parts.append(doc.crop(pi, c["bbox"], ck))
                parts += [caption_md(cap) for cap in caps]
                if b["type"] == "table" and caps:  # a table's caption sits above it
                    parts = parts[len(body):] + parts[:len(body)]
                items.append(("raw", "\n\n".join(parts), ref))
                # a figure floats: a paragraph it interrupts still joins its continuation after it
            elif t == "ref_text":
                items.append(("ref", spans_md(b["content"])))
            else:
                text = spans_md(b["content"]) if isinstance(b.get("content"), list) else str(b.get("content", ""))
                if text:
                    items.append(("p", text, ref))
                    last_p = None
    return doc, doc_title, byline, numbered_doc, items


def sectionize(doc_title, numbered_doc, items, single):
    """Split the item list into sections at top-level headings (long numbered papers only); references go to a
    separate verbatim section."""
    refs = [i[1] for i in items if i[0] == "ref"]
    body = [i for i in items if i[0] != "ref" and not (i[0] == "h" and BACK_TITLES.match(i[2]))]
    total = sum(words(i[1] if i[0] != "h" else i[2]) for i in body)
    split = numbered_doc and total > SPLIT_WORDS and not single
    sections = [{"title": doc_title, "items": []}]
    for it in body:
        if split and it[0] == "h" and it[1] == 1 and NUMBERED.match(it[2]):
            sections.append({"title": it[2], "items": []})
        else:
            sections[-1]["items"].append(it)
    merged = [sections[0]]
    for s in sections[1:]:
        n = sum(words(i[1] if i[0] != "h" else i[2]) for i in s["items"])
        if n < MIN_SECTION_WORDS:
            merged[-1]["items"] += [("h", 1, s["title"], "")] + s["items"]
        else:
            merged.append(s)
    return merged, refs


def split_large(sections, limit):
    """Splits a section over `limit` words (a ⟦IMG⟧/⟦CODE⟧ token counting as 25) so each translation call stays small. Its items are grouped into runs
    that each start at a heading; runs are packed into pieces of at most `limit` words, each later piece titled by
    the heading it starts at. A single run over 1.5 x `limit` is cut at paragraph boundaries, the later parts
    titled "<heading> (continued)"."""
    # a placeholder weighs like 25 words: a section of 188 code blocks overwhelms the translator long before its
    # prose does, and it starts dropping tokens
    size = lambda it: words(it[2] if it[0] == "h" else it[1]) + (0 if it[0] == "h" else 25 * it[1].count("⟦"))
    out = []
    for s in sections:
        if sum(size(it) for it in s["items"]) <= limit:
            out.append(s)
            continue
        runs = [[s["title"], []]]
        for it in s["items"]:
            if it[0] == "h" and runs[-1][1]:
                runs.append([it[2], []])
            runs[-1][1].append(it)
        piece, n = None, 0
        for title, items in runs:
            m = sum(size(it) for it in items)
            if piece is None or n + m > limit:
                if piece:
                    out.append(piece)
                piece, n = {"title": title, "items": []}, 0
            for it in items:
                if n > 1.5 * limit and it[0] != "h":
                    out.append(piece)
                    piece, n = {"title": f"{title} (continued)", "items": []}, 0
                piece["items"].append(it)
                n += size(it)
        out.append(piece)
    return out


def section_md(s):
    """A section's Markdown: '# title', its blocks, then its footnotes after a rule."""
    out, refs, notes = [f"# {s['title']}"], [], []
    for it in s["items"]:
        kind = it[0]
        if kind == "h":
            out.append(("## " if it[1] == 1 else "### ") + it[2])
            refs.append(it[3])
        elif kind == "note":
            notes.append(it[1])
            refs.append(it[2])
        else:
            out.append(it[1])
            refs.append(it[2])
    if notes:
        out.append("---")
        out += notes
    return "\n\n".join(out) + "\n", "\n\n".join(r for r in refs if r)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pdf")
    ap.add_argument("work")
    ap.add_argument("--mineru", default=os.environ.get("MINERU_KIT") or str(DEFAULT_MINERU))
    ap.add_argument("--single", action="store_true", help="keep the whole document as one section")
    opt = ap.parse_args()
    pdf, work = Path(opt.pdf).resolve(), Path(opt.work).resolve()
    work.mkdir(parents=True, exist_ok=True)
    middle = run_mineru(pdf, work, opt.mineru)
    for page in middle["pages"]:
        page.setdefault("blocks", [])  # a blank page comes with no block list
    doc, doc_title, byline, numbered_doc, items = convert(pdf, work, middle)
    sections, refs = sectionize(doc_title, numbered_doc, items, opt.single)
    sections = split_large(sections, MAX_SECTION_WORDS)

    (work / "src").mkdir(exist_ok=True)
    (work / "translated").mkdir(exist_ok=True)
    meta_sections = []
    for i, s in enumerate(sections, 1):
        sid = f"{i:02d}"
        md, ref = section_md(s)
        (work / "src" / f"{sid}.md").write_text(md)
        (work / "src" / f"{sid}.ref.txt").write_text(ref)
        meta_sections.append({"id": sid, "title": s["title"], "kind": "chapter", "file": f"src/{sid}.md", "words": words(md)})
    if refs:
        sid = f"{len(sections) + 1:02d}"
        (work / "src" / f"{sid}.md").write_text("# References\n")
        (work / "translated" / f"{sid}.md").write_text("# 参考文献\n\n" + "\n\n".join(refs) + "\n")
        meta_sections.append({"id": sid, "title": "References", "kind": "back", "file": f"src/{sid}.md", "words": 0, "verbatim": True})

    our_col = PAGE_SIZE[0] * (1 - 2 * SIDE_MARGIN)
    meta = {"title": doc_title, "author": ", ".join(middle["metadata"].get("document", {}).get("authors") or []),
            "byline": byline, "page_size": PAGE_SIZE, "source_kind": "paper", "source_pdf": str(pdf),
            # figure crops display at their original size scaled from the source's text column to ours
            "img_scale": round(our_col / doc.text_col / CROP_ZOOM, 4), "sections": meta_sections}
    (work / "sections.json").write_text(json.dumps(meta, ensure_ascii=False, indent=1))
    (work / "images.json").write_text(json.dumps(doc.images, indent=1))
    (work / "codeblocks.json").write_text(json.dumps(doc.codes, indent=1, ensure_ascii=False))
    print(f"{work}: {doc_title!r}, body {doc.body_size}pt, column {doc.text_col:.0f}pt, {len(doc.images)} figures, {len(doc.codes)} code blocks")
    for s in meta_sections:
        print(f"  {s['id']} {s['kind']} {s['title']}: {s['words']}")


if __name__ == "__main__":
    main()
