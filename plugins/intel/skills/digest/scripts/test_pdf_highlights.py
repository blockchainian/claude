#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Tests digest's PDF flow: splitting a PDF into chapters (bookmarks) or pages, and (when Chrome is
# ABOUTME: present) a real render of per-chapter highlights into a PDF with the source's page size.
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace

import pikepdf

HERE = Path(__file__).parent
fails = []


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


def make_pdf(path, pages, outline=(), size=(468, 680)):
    """A PDF with one line of text per page and a flat outline of (title, zero-based page) bookmarks."""
    pdf = pikepdf.new()
    font = pdf.make_indirect(pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1,
                                                BaseFont=pikepdf.Name.Helvetica))
    for text in pages:
        page = pdf.add_blank_page(page_size=size)
        page.Resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=font))
        page.Contents = pdf.make_stream(f"BT /F1 12 Tf 40 600 Td ({text}) Tj ET".encode())
    if outline:
        with pdf.open_outline() as o:
            for title, index in outline:
                o.root.append(pikepdf.OutlineItem(title, index))
    pdf.docinfo["/Title"] = "A Small Book"
    pdf.save(path)


PAGES = ["", "Opening words of the first chapter", "More of the first chapter", "The second chapter starts here",
         "The third chapter is one page"]
OUTLINE = [("Cover", 0), ("One: Beginnings", 1), ("Two: Middles", 3), ("Three: Ends", 4)]


def test_split(ph, tmp):
    book = tmp / "small-book.pdf"
    make_pdf(book, PAGES, OUTLINE)
    by_page = ph.split(str(book), by="page")
    check("--by page splits per page", by_page["unit"] == "page" and len(by_page["chapters"]) == len(PAGES),
          (by_page["unit"], len(by_page["chapters"])))

    meta = ph.split(str(book), by=None)
    work = Path(meta["work"])
    chapters = meta["chapters"]
    check("bookmarked PDF splits by chapter", meta["unit"] == "chapter", meta["unit"])
    check("one chapter per bookmark", [c["title"] for c in chapters] == [t for t, _ in OUTLINE],
          [c["title"] for c in chapters])
    check("chapter page ranges are 1-based and contiguous", [c["pages"] for c in chapters] == [[1, 1], [2, 3], [4, 4], [5, 5]],
          [c["pages"] for c in chapters])
    one = (work / chapters[1]["text"]).read_text()
    check("a chapter's text spans its pages only", "Opening words" in one and "More of the first" in one
          and "second chapter" not in one, one[:200])
    check("an empty chapter reports zero chars", chapters[0]["chars"] == 0, chapters[0]["chars"])
    check("page size comes from the source", meta["page_size"] == [468, 680], meta["page_size"])
    check("title comes from the PDF metadata", meta["title"] == "A Small Book", meta["title"])
    check("chapters.json is written", json.loads((work / "chapters.json").read_text())["slug"] == "small-book")
    check("highlights paths are per chapter", chapters[1]["highlights"] == "md/02.md", chapters[1]["highlights"])

    plain = tmp / "no-outline.pdf"
    make_pdf(plain, PAGES[1:], size=(612, 792))
    meta2 = ph.split(str(plain), by=None)
    check("PDF without bookmarks falls back to pages", meta2["unit"] == "page" and len(meta2["chapters"]) == 4,
          (meta2["unit"], len(meta2["chapters"])))
    check("page sections are titled by page number", meta2["chapters"][2]["title"] == "Page 3", meta2["chapters"][2]["title"])
    check("fallback reads the source page size", meta2["page_size"] == [612, 792], meta2["page_size"])

    scanned = tmp / "scanned.pdf"
    make_pdf(scanned, ["", ""])
    try:
        ph.split(str(scanned), by=None)
        check("image-only PDF is refused", False)
    except SystemExit as e:
        check("image-only PDF is refused", "scanned" in str(e), str(e))
    return work


def test_render(ph, work, tmp):
    chrome = None
    for c in [os.environ.get("CHROME"), "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
              shutil.which("google-chrome"), shutil.which("chromium")]:
        if c and Path(c).exists():
            chrome = c
    if not chrome:
        print("SKIP: render e2e (Chrome missing)")
        return
    (work / "md").mkdir(exist_ok=True)
    (work / "md" / "02.md").write_text("# One: Beginnings\n\n## Theme\n\n- The opening point about **beginnings**.\n")
    (work / "md" / "04.md").write_text("## Theme\n\n- The closing point about ends.\n")
    out = tmp / "out.pdf"
    opt = SimpleNamespace(out=str(out), bg="#000409", fg="#6e7f7a", font_size=9.25, bold_factor=1.25)
    ph.render(work, opt)
    check("highlights PDF written", out.is_file())
    pdf = pikepdf.open(out)
    box = [float(v) for v in pdf.pages[0].mediabox]
    check("output page size matches the source", abs(box[2] - 468) < 1 and abs(box[3] - 680) < 1, box)
    with pdf.open_outline() as o:
        titles = [i.title for i in o.root]
    check("one bookmark per highlighted chapter, in order", titles == ["One: Beginnings", "Three: Ends"], titles)
    first = pdf.pages[0].Contents
    stream = (first[0] if isinstance(first, pikepdf.Array) else first).read_bytes()
    check("pages are underlaid with the background colour", b" rg " in stream and b" re f" in stream, stream[:80])
    text = subprocess.run(["pdftotext", "-layout", str(out), "-"], capture_output=True, text=True).stdout
    check("highlights text is typeset", "opening point about beginnings" in text and "closing point about ends" in text, text[:300])
    check("a chapter without a title line takes the bookmark's", "Three: Ends" in text, text[:300])
    check("each chapter opens on its own page", len(pdf.pages) == 2, len(pdf.pages))
    draft = (work / "draft.md").read_text()
    check("a store-ready draft is written", draft.startswith("---\ntitle: A Small Book\n") and "\nurl: " in draft
          and "## One: Beginnings" in draft and "### Theme" in draft and "## Three: Ends" in draft, draft[:300])

    meta_path = work / "chapters.json"
    meta = json.loads(meta_path.read_text())
    meta["cover"] = "Jane Doe"
    meta_path.write_text(json.dumps(meta))
    ph.render(work, opt)
    pdf = pikepdf.open(out)
    check("a cover name adds one cover page in front", len(pdf.pages) == 3, len(pdf.pages))
    cover_text = subprocess.run(["pdftotext", "-l", "1", str(out), "-"], capture_output=True, text=True).stdout
    check("the cover shows the name and the book title", "Jane Doe" in cover_text and "A Small Book" in cover_text, cover_text[:200])
    with pdf.open_outline() as o:
        items = [(i.title, pdf.pages.index(pikepdf.Page(i.destination[0]))) for i in o.root]
    check("bookmarks follow the chapters behind the cover", items == [("Jane Doe", 0), ("One: Beginnings", 1), ("Three: Ends", 2)], items)
    meta["cover"] = "A Small"
    meta_path.write_text(json.dumps(meta))
    ph.render(work, opt)
    cover_text = subprocess.run(["pdftotext", "-l", "1", str(out), "-"], capture_output=True, text=True).stdout
    check("a title that starts with the name is not repeated under it", cover_text.split() == ["A", "Small", "Book"], cover_text[:200])
    meta.pop("cover")
    meta_path.write_text(json.dumps(meta))

    default = ph.default_out(Path(json.loads((work / "chapters.json").read_text())["pdf"]), work)
    check("default output sits next to the source", default == tmp / "small-book-highlights.pdf", str(default))


def main():
    if not shutil.which("pdftotext"):
        print("SKIP: all (pdftotext missing; run setup.sh)")
        return
    tmp = Path(tempfile.mkdtemp()).resolve()
    os.environ["HIGHLIGHTS_DIR"] = str(tmp / "store")
    ph = load("pdf_highlights")
    work = test_split(ph, tmp)
    test_render(ph, work, tmp)
    print()
    if fails:
        print(f"{len(fails)} FAILED: {fails}")
        sys.exit(1)
    print("ALL PASSED")


if __name__ == "__main__":
    main()
