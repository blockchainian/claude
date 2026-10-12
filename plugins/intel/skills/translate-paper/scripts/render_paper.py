#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Renders a translated paper work dir with the translate-epub skill's render.py, adding what a paper needs: figure
# ABOUTME: crops shown at their source size, footnotes set small, and a cover that carries the English title.
#
# Usage: render_paper.py <work dir> --out <paper.pdf> [render.py options: --only, --bg, --fg, --font-size, --no-recolor, --no-dropcap]
# Lecture or recitation notes (no byline) get no cover; a one-section document gets no 目录.
import importlib.util
import re
import sys
from pathlib import Path

spec = importlib.util.spec_from_file_location("render", Path(__file__).resolve().parents[2] / "translate-epub" / "scripts" / "render.py")
render = importlib.util.module_from_spec(spec)
spec.loader.exec_module(render)

# Footnotes small under a rule; links in the house link colour (nvim's dark-mode accent, as the English PDFs use);
# list items flush left with no first-line indent, since justifying a short item around a formula spaces out its
# Chinese characters.
NOTES_CSS = """
.notes { margin-top: 14pt; padding-top: 6pt; border-top: 0.5pt solid var(--fg); width: 100%; font-size: 0.82em; line-height: 1.55; }
.notes p { text-indent: 0; margin: 0 0 3pt; }
a { color: #005f87; text-decoration: none; }
.refs p { text-indent: -2em; padding-left: 2em; margin-bottom: 3pt; text-align: left; }
li p { text-indent: 0; }
li, li p { text-align: left; }
"""


def author_lines(meta):
    """The cover's author lines: one author per line, then the shared affiliation on the last line; when authors
    are at different places, each line is "name, affiliation". Falls back to the raw byline."""
    cover = meta.get("cover_authors")
    if not cover:
        return meta.get("byline", [])
    authors = cover["authors"]
    if cover.get("shared"):
        return [a["name"] for a in authors] + [cover["shared"]]
    return [f'{a["name"]}, {a["affiliation"]}' if a.get("affiliation") else a["name"] for a in authors]


def title_cover(meta, bg, fg, chrome, tmp):
    """A one-page cover with the paper's English title and byline (authors, affiliations), centred in the heading
    face and ink (render.py's build_cover shows a book's cover image; a paper's first page would put the English
    original before the translation)."""
    w, h = meta["page_size"]
    fg = render.brighten(fg, 1.25)
    byline = "".join(f"<p>{render.html.escape(line)}</p>" for line in author_lines(meta))
    out = tmp / "cover.pdf"
    (tmp / "cover.html").write_text(
        f'<!doctype html><html><head><meta charset="utf-8"><style>@page {{ size: {w}pt {h}pt; margin: 0; }}'
        f'html,body {{ margin:0; height:100%; background:{bg}; }}'
        f'body {{ display:flex; align-items:center; justify-content:center; }}'
        f'h1 {{ font-family: Baskerville, serif; font-weight: 700; color:{fg}; font-size: 26pt; line-height: 1.3;'
        f' text-align:center; margin: 0 0 22pt; }}'
        f'.c {{ margin: 0 {round(w * 0.135)}pt; padding-bottom: {round(h * 0.12)}pt; text-align:center; }}'
        f'p {{ font-family: Baskerville, "Songti SC", serif; color:{fg}; font-size: 10pt; line-height: 1.5; margin: 0 0 6pt; }}'
        f'</style></head><body><div class="c"><h1>{render.html.escape(meta.get("title", ""))}</h1>{byline}</div></body></html>')
    render.print_pdf(chrome, tmp / "cover.html", out)
    return out


def drop_bookmarked(out, title):
    """Removes the pages a top-level bookmark covers (from its page to the next bookmark's page) and the bookmark.
    Used for the 目录 of a one-section document, where one line of contents is noise, and for the cover of
    lecture notes, which carry no title page of their own (both the user's calls)."""
    pikepdf = render.pikepdf
    pdf = pikepdf.open(out, allow_overwriting_input=True)

    def page_of(item):
        dest = item.destination
        return dest if isinstance(dest, int) else pdf.pages.index(pikepdf.Page(dest[0]))

    with pdf.open_outline() as outline:
        items = list(outline.root)
        toc = next((k for k, item in enumerate(items) if item.title == title), None)
        if toc is None:
            return
        start = page_of(items[toc])
        end = page_of(items[toc + 1]) if toc + 1 < len(items) else start + 1
        del outline.root[toc]
    for k in range(end - 1, start - 1, -1):
        del pdf.pages[k]
    pdf.save(out)


def main():
    ap = render.argparse.ArgumentParser()
    ap.add_argument("work")
    ap.add_argument("--out")
    ap.add_argument("--only")
    ap.add_argument("--title")
    render.add_style_args(ap)
    ap.add_argument("--no-recolor", dest="recolor", action="store_false")
    ap.add_argument("--no-dropcap", dest="dropcap", action="store_false")
    opt = ap.parse_args()
    work = Path(opt.work).resolve()
    meta = render.json.loads((work / "sections.json").read_text())
    fg = render.page_colors(opt)[1]
    render.build_cover = lambda meta, work, bg, chrome, tmp: title_cover(meta, bg, fg, chrome, tmp)

    # A section's footnotes follow its "---" line: set them small under a rule, as a paper prints them.
    to_html = render.md_to_html

    def md_to_html(md_text, codes=None):
        title, body = to_html(md_text, codes)
        return title, re.sub(r"<hr\s*/?>(.*)$", r'<div class="notes">\1</div>', body, count=1, flags=re.S)

    render.md_to_html = md_to_html
    css = render.css
    render.css = lambda *a, **k: css(*a, **k) + NOTES_CSS

    # The references section: no drop cap, entries with a hanging indent.
    section_html = render.section_html

    def refs_aware_section_html(sec, title, body, dropcap=True):
        if not sec.get("verbatim"):
            return section_html(sec, title, body, dropcap)
        return section_html(sec, title, body, False).replace('class="body-text', 'class="refs body-text', 1)

    render.section_html = refs_aware_section_html

    # Figure crops: every block image at its intrinsic width times img_scale (source size mapped to our column).
    place = render.place_images
    render.place_images = lambda body, w, images, fg, bg, eq, inl, recolor=True: place(body, w, images, fg, bg, meta.get("img_scale"), None, recolor)

    if not opt.title:
        first = next((s for s in meta["sections"] if s.get("file")), None)
        md = work / "translated" / (Path(first["file"]).stem + ".md") if first else None
        if md and md.exists():
            opt.title = re.sub(r"^#\s*", "", md.read_text().splitlines()[0]).strip()
    if not opt.out and not opt.only:
        sys.exit("--out is required for a whole-paper render")
    render.render(work, opt)
    if opt.out and not meta.get("byline"):  # lecture or recitation notes, not a paper: no cover
        drop_bookmarked(Path(opt.out), "Cover")
    if opt.out and sum(1 for sec in meta["sections"] if sec.get("file")) == 1:
        drop_bookmarked(Path(opt.out), "目录")


if __name__ == "__main__":
    main()
