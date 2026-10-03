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

HERE = Path(__file__).resolve().parent.parent / "scripts"
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

    chart = ("```chart\ntype: line\ntitle: Subscribers by year\ncolumns: Date | Jane | Rival\n2012-07-11 | 1,002,877 | 110,010\n"
             "2013-07-09 | 10,058,670 | 681,229\n2014-01-09 | 20,010,912 | 1,413,462\n```")
    table = "```chart\ntype: table\ntitle: Income by year\ncolumns: Year | Estimate\n2015 | $12 million\n2016 | $15 million\n```"
    (work / "md" / "04.md").write_text(f"## Theme\n\n- The closing point about ends.\n\n{chart}\n\nAfter the chart.\n\n{table}\n")
    ph.render(work, opt)
    text = subprocess.run(["pdftotext", "-layout", str(out), "-"], capture_output=True, text=True).stdout
    # the paragraph after the chart is the chapter's first, so its first letter is set apart as a drop cap
    check("a chart block is drawn, not printed as text", "Subscribers by year" in text and "20,010,912" in text and "Rival" in text
          and "```" not in text and "type: line" not in text and "fter the chart." in text, [k for k in ("Subscribers by year", "20,010,912", "Rival", "fter the chart.") if k not in text] + [k for k in ("```", "type: line") if k in text])
    check("a table block is typeset as a table", "Income by year" in text and "$15 million" in text and "type: table" not in text, text[-400:])
    (work / "md" / "04.md").write_text("## Theme\n\n- The closing point about ends.\n")
    ph.render(work, opt)

    meta_path = work / "chapters.json"
    meta = json.loads(meta_path.read_text())
    meta["cover"] = "Jane Doe"
    meta_path.write_text(json.dumps(meta))
    ph.render(work, opt)
    pdf = pikepdf.open(out)
    check("a cover name adds one cover page in front", len(pdf.pages) == 3, len(pdf.pages))
    cover_text = subprocess.run(["pdftotext", "-l", "1", str(out), "-"], capture_output=True, text=True).stdout
    check("the cover shows the name and nothing else", cover_text.split() == ["Jane", "Doe"], cover_text[:200])
    with pdf.open_outline() as o:
        items = [(i.title, pdf.pages.index(pikepdf.Page(i.destination[0]))) for i in o.root]
    check("bookmarks follow the chapters behind the cover", items == [("Jane Doe", 0), ("One: Beginnings", 1), ("Three: Ends", 2)], items)
    meta["accounts"] = ["https://www.youtube.com/@JaneDoe", "https://x.com/janedoe"]
    meta_path.write_text(json.dumps(meta))
    (work / "md" / "04.md").write_text("## Theme\n\n- The Daily: [2017](https://a.example/2017)\n")
    ph.render(work, opt)
    pdf = pikepdf.open(out)
    uris = lambda page: sorted(str(a.A.URI) for a in page.get("/Annots", []) if "/A" in a and "/URI" in a.A)
    cover_text = subprocess.run(["pdftotext", "-l", "1", str(out), "-"], capture_output=True, text=True).stdout
    check("the cover lists the subject's accounts by handle under the name", cover_text.split() == ["Jane", "Doe", "@JaneDoe", "@janedoe"], cover_text[:200])
    check("each account on the cover is a link to it", uris(pdf.pages[0]) == ["https://www.youtube.com/@JaneDoe", "https://x.com/janedoe"], uris(pdf.pages[0]))
    check("a link in a chapter is a link in the PDF", uris(pdf.pages[2]) == ["https://a.example/2017"], uris(pdf.pages[2]))
    (work / "md" / "04.md").write_text("## Theme\n\n- The closing point about ends.\n")
    known = ph.account_html("https://www.tiktok.com/@jane")
    check("an account on a known platform gets its logo, its handle and its link", "<svg" in known and 'href="https://www.tiktok.com/@jane"' in known and ">@jane<" in known, known[:200])
    other = ph.account_html("https://blog.example/jane/")
    check("an account elsewhere is shown by its address, without a logo", "<svg" not in other and ">blog.example/jane<" in other, other)
    meta.pop("cover")
    meta.pop("accounts")
    meta_path.write_text(json.dumps(meta))

    default = ph.default_out(Path(json.loads((work / "chapters.json").read_text())["pdf"]), work)
    check("default output sits next to the source", default == tmp / "small-book-highlights.pdf", str(default))


def test_charts(charts):
    line = charts.figures("Before.\n\n```chart\ntype: line\ntitle: Subscribers\n2012 | 1,002,877\n2013 | 10,058,670\n2014 | 20,010,912\n```\n\nAfter.")
    check("a line chart block becomes one figure with an inline drawing", line.count("<figure") == 1 and "<svg" in line and "<polyline" in line
          and "Subscribers" in line and "20,010,912" in line and "```" not in line and line.startswith("Before.") and line.endswith("After."), line[:300])
    rows = "\n".join(f"{2000 + i} | {(i + 1) * 1111}" for i in range(12))
    long = charts.figures(f"```chart\ntype: line\ntitle: T\n{rows}\n```")
    check("a long series prints only its first, last and highest value", long.count("<circle") == 2 and "1111<" in long and "13332<" in long and "5555<" not in long, long[-400:])
    bar = charts.figures("```chart\ntype: bar\ntitle: Views in four days\nMontage | 2,520,866\nOrdinary upload | 1,261,237\n```")
    check("a bar chart block draws one bar per row with its value", bar.count("<rect") == 2 and "Montage" in bar and "2,520,866" in bar, bar[:300])
    log = charts.figures("```chart\ntype: line\nscale: log\ntitle: T\n2010 | 19\n2012 | 1,002,877\n2019 | 100,020,115\n```")
    check("a log scale keeps a small first value off the baseline", "<polyline" in log and "19" in log, log[:200])
    units = charts.figures("```chart\ntype: bar\ntitle: T\nA | 1.11 亿\nB | 5,000 万\nC | 20M\n```")
    widths = [float(w) for w in __import__("re").findall(r'<rect[^>]* width="([\d.]+)"', units)]
    check("values with a unit (万, 亿, K, M, B) are drawn to scale and printed as written", "1.11 亿" in units and len(widths) == 3
          and abs(widths[1] / widths[0] - 50 / 111) < 0.01 and abs(widths[2] / widths[0] - 20 / 111) < 0.01, str(widths))
    table = charts.figures("```chart\ntype: table\ntitle: Income\ncolumns: Year | Estimate\n2015 | $12 million\n```")
    check("a table block becomes an HTML table with its header", "<table" in table and "<th>Year</th>" in table and "$12 million" in table, table[:300])
    for bad, why in [("```chart\ntype: pie\nA | 1\n```", "an unknown type"), ("```chart\ntype: line\ntitle: T\n2012 | many\n2013 | 3\n```", "a value that is not a number"),
                     ("```chart\ntype: bar\ntitle: T\n```", "no rows")]:
        try:
            charts.figures(bad)
            check(f"a chart block with {why} stops the render", False)
        except SystemExit as stop:
            check(f"a chart block with {why} stops the render", "chart" in str(stop), str(stop))
    check("text without chart blocks is returned unchanged", charts.figures("Plain.\n\n- point\n") == "Plain.\n\n- point\n")


def main():
    test_charts(load("charts"))
    if not shutil.which("pdftotext"):
        print("SKIP: all (pdftotext missing; run setup.sh)")
        return
    tmp = Path(tempfile.mkdtemp()).resolve()
    os.environ["DIGESTS_DIR"] = str(tmp / "store")
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
