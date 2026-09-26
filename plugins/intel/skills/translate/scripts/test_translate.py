#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Tests the translate skill: section derivation, OCR text cleanup, prompt/answer checks, Markdown
# ABOUTME: typesetting, and (when Chrome is present) a real render of a small generated book with cover and bookmarks.
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


def test_extract(ex):
    check("chinese numbers", [ex.chinese_number(n) for n in (1, 10, 11, 20, 24, 105)] == ["一", "十", "十一", "二十", "二十四", "一百零五"])
    check("parse '1. Title'", ex.parse_title("1. Traction Channels") == (1, "Traction Channels"))
    check("parse 'Chapter Twelve: X'", ex.parse_title("Chapter Twelve: SEO") == (12, "SEO"))
    check("parse plain title", ex.parse_title("Preface: Traction Trumps Everything") == (None, "Preface: Traction Trumps Everything"))
    entries = [("COVER", 0), ("CONTENTS", 5), ("Preface", 9), ("1. One", 17), ("2. Two", 24), ("Acknowledgments", 30), ("Index", 33)]
    secs = ex.build_sections(entries, 40)
    kinds = [s["kind"] for s in secs]
    check("section kinds", kinds == ["cover", "contents", "front", "chapter", "chapter", "back", "skip"], str(kinds))
    check("section ranges", [(s["start"], s["end"]) for s in secs][2:5] == [(10, 17), (18, 24), (25, 30)])
    check("chapter label", secs[3]["label"] == "第一章" and secs[2]["label"] is None)

    pages = ["      PREFACE: TRACTION TRUMPS EVERYTHING\n\n"
             "    n 2006 I sold a company. It was strange for many rea-\n"
             "sons, not the least of which was that we had no employees.\n"
             "     The terms were such that I moved twenty-\n"
             "five miles away.\n"
             "         Traction is basically quantitative evidence\n"
             "         of customer demand, says Naval.\n"
             "\n"
             "         So if you are in enterprise software the bar\n"
             "         is low.\n"
             "     Next paragraph starts here\n"
             "                    ix\n",
             "      TRACTION\n"
             "and continues on the next page.\n"
             "     A fresh paragraph.\n"
             "                     10\n"]
    paras = ex.clean_pages(pages, ["Preface: Traction Trumps Everything", "Traction"])
    check("running heads and folios dropped", not any("PREFACE" in p or p.strip() in ("ix", "10") or p == "TRACTION" for p in paras), str(paras))
    check("hyphenated word rejoined", any("reasons" in p for p in paras), str(paras))
    check("real hyphen kept", any("twenty-five" in p for p in paras), str(paras))
    check("indented block becomes one quote", sum(p.startswith("> ") for p in paras) == 1 and any(p.startswith("> Traction is basically") and "the bar is low" in p for p in paras), str(paras))
    check("paragraph continues across pages", any(p.startswith("Next paragraph starts here and continues") for p in paras), str(paras))
    check("paragraph count", len(paras) == 5, str(len(paras)) + " " + str(paras))


def test_translate(tr):
    meta = {"title": "Traction", "author": "Weinberg"}
    sec = {"id": "04", "title": "Traction Channels", "label": "第一章", "kind": "chapter", "words": 10, "file": "text/04-x.txt"}
    prompt = tr.build_prompt(meta, sec, "Hello world.", "traction → 牵引力")
    check("prompt names book, section and glossary", "Book: Traction by Weinberg" in prompt and "第一章 Traction Channels" in prompt
          and "traction → 牵引力" in prompt and prompt.rstrip().endswith("Hello world."))
    check("rules forbid tools and demand the title line", "do not run commands" in tr.RULES and '"# "' in tr.RULES)
    check("answer without title rejected", tr.check_output("正文而已", 5) is not None)
    check("short answer rejected", tr.check_output("# 标题\n\n短", 100) is not None)
    check("good answer accepted", tr.check_output("# 标题\n\n" + "汉" * 200, 100) is None)
    with tempfile.TemporaryDirectory() as d:
        home = tr.codex_home(Path(d), "gpt-6-luna", "low", "/x/instructions.md", "priority")
        cfg = (home / "config.toml").read_text()
        check("codex config pins model, effort, tier", 'model = "gpt-6-luna"' in cfg and 'model_reasoning_effort = "low"' in cfg
              and 'service_tier = "priority"' in cfg and "project_doc_max_bytes = 0" in cfg)
        home = tr.codex_home(Path(d) / "b", "gpt-6-luna", "low", "/x/i.md", "standard")
        check("standard tier omits service_tier", "service_tier" not in (home / "config.toml").read_text())


def test_render_units(rd):
    if (Path.home() / "Library/Preferences/com.googlecode.iterm2.plist").exists():
        bg, fg = rd.iterm_colors()
        check("iterm colors are hex", bg.startswith("#") and len(bg) == 7 and fg.startswith("#") and len(fg) == 7, f"{bg} {fg}")
    title, body = rd.md_to_html("# 章名\n\n第一段 *强调*。\n\n> 引文\n\n## 小标题\n\n第二段。\n")
    check("markdown title split off", title == "章名" and "<h1" not in body)
    check("markdown body html", "<em>强调</em>" in body and "<blockquote>" in body and "<h2>小标题</h2>" in body)
    check("roman folios", [rd.folio_for(i, None) for i in range(3)] == ["i", "ii", "iii"])
    style = rd.css([427.6, 660], "#181a1d", "#e1ddd5", [("03", "前言", "front"), ("04", "第一章", "chapter")])
    check("css page size and colors", "size: 427.6pt 660pt" in style and "--bg: #181a1d" in style and "Baskerville" in style)
    check("css front roman, body arabic", '@page s03 { @top-center { content: "前言"' in style and "counter(page, lower-roman)" in style.split("@page s03")[1].split("}}")[0]
          and "content: counter(page);" in style.split("@page s04")[1].split("} }")[0])
    sec = {"id": "04", "kind": "chapter", "label": "第一章"}
    frag = rd.section_html(sec, "标题", "<p>x</p>")
    check("section carries marker, label, named page", "⟦S04⟧" in frag and "第一章" in frag and 'page: s04' in frag)
    toc = rd.contents_html([(sec, "标题")], {"04": "1"})
    check("contents row", "⟦TOC⟧" in toc and '<span class="pg">1</span>' in toc)


def make_source_book(path, chrome):
    """A three-page 'English' book with an outline (Cover, Preface, 1. One) printed by Chrome, cover page colored."""
    html_path = path.with_suffix(".html")
    html_path.write_text('<!doctype html><meta charset="utf-8"><style>@page{size:427.6pt 660pt;margin:0}'
                         'body{margin:0;background:#181a1d;color:#e1ddd5;font:12pt Baskerville}'
                         '.p{height:660pt;padding:60pt;box-sizing:border-box;break-after:page}</style>'
                         '<div class="p" style="background:#2244aa">COVER</div><div class="p">Preface text here.</div>'
                         '<div class="p">Chapter one text here.</div>')
    subprocess.run([chrome, "--headless=new", "--disable-gpu", "--no-pdf-header-footer", f"--print-to-pdf={path}",
                    f"file://{html_path}"], check=True, capture_output=True)
    pdf = pikepdf.open(path, allow_overwriting_input=True)
    with pdf.open_outline() as outline:
        outline.root.append(pikepdf.OutlineItem("COVER", 0))
        outline.root.append(pikepdf.OutlineItem("Preface", 1))
        outline.root.append(pikepdf.OutlineItem("1. One", 2))
    pdf.docinfo["/Title"] = "Tiny Book: A Test"
    pdf.docinfo["/Author"] = "Tester"
    pdf.save(path)


def test_render_e2e(rd):
    chrome = None
    for c in [os.environ.get("CHROME"), "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
              shutil.which("google-chrome"), shutil.which("chromium")]:
        if c and Path(c).exists():
            chrome = c
    if not chrome or not shutil.which("pdftotext"):
        print("SKIP: render e2e (Chrome or pdftotext missing)")
        return
    with tempfile.TemporaryDirectory() as d:
        book = Path(d) / "tiny.pdf"
        make_source_book(book, chrome)
        work = Path(d) / "work"
        r = subprocess.run([sys.executable, str(HERE / "extract.py"), str(book), "--work", str(work)], capture_output=True, text=True)
        check("extract runs on generated book", r.returncode == 0, r.stderr[-500:])
        meta = json.loads((work / "sections.json").read_text())
        check("extract finds cover, front, chapter", [s["kind"] for s in meta["sections"]] == ["cover", "front", "chapter"], str(meta["sections"]))
        check("extract page size from source", meta["page_size"][1] == 660.0, str(meta["page_size"]))
        (work / "md").mkdir()
        for s in meta["sections"]:
            if s.get("file"):
                (work / "md" / (Path(s["file"]).stem + ".md")).write_text(f"# {s['title']}译\n\n" + ("正文。" * 400 + "\n\n") * 6)
        out = Path(d) / "tiny-zh.pdf"
        opt = SimpleNamespace(only=None, out=str(out), title="小书", bg=None, fg=None, font_size=9.25)
        rd.render(work, opt)
        pdf = pikepdf.open(out)
        with pdf.open_outline() as outline:
            marks = [(str(i.title), pikepdf.Page(i.destination[0]).index) for i in outline.root]
        check("bookmarks: Cover, 目录, sections", [m[0] for m in marks] == ["Cover", "目录", "Preface译", "第一章　One译"], str(marks))
        check("bookmark pages ascend from the cover", marks[0][1] == 0 and marks[1][1] == 1 and marks[2][1] == 2 < marks[3][1], str(marks))
        check("title metadata", str(pdf.docinfo["/Title"]) == "小书" and str(pdf.docinfo["/Author"]) == "Tester")
        text = subprocess.run(["pdftotext", "-layout", str(out), "-"], capture_output=True, text=True).stdout.split("\f")
        folios = [ln.strip() for pg in text for ln in pg.splitlines() if ln.strip() in ("i", "ii", "iii", "iv", "1", "2", "3", "4")]
        check("front roman then body arabic from 1", folios[:2] == ["i", "ii"] and "1" in folios and folios.index("1") > folios.index("ii"), str(folios))
        check("contents lists body page 1", any("One译" in ln and ln.rstrip().endswith("1") for ln in text[1].splitlines()), text[1])
        src = pikepdf.open(book)
        src_box = [float(v) for v in src.pages[0].mediabox]
        cover_box = [float(v) for v in pdf.pages[0].mediabox]
        check("cover page copied from the source", cover_box == src_box and "COVER" in text[0], f"{cover_box} vs {src_box}")
        opt = SimpleNamespace(only=meta["sections"][2]["id"], out=None, title=None, bg="#ffffff", fg="#000000", font_size=12)
        rd.render(work, opt)
        check("single-section preview written", any((work / "pdf").glob("*.pdf")))


def test_luna_e2e(tr):
    if not os.environ.get("TRANSLATE_E2E"):
        print("SKIP: Luna e2e (set TRANSLATE_E2E=1 to spend one gpt-6-luna call)")
        return
    md, usage, seconds = tr.run_codex("Book: Test\nSection: Preface (front, 12 words)\n\nSource text:\n\nTraction is a sign that your company is taking off. Nothing else matters.\n",
                                     "gpt-6-luna", "low", "priority")
    check("luna returns a titled Chinese translation", tr.check_output(md, 12) is None and "牵引力" in md, md[:200])
    check("luna usage reported", usage and usage.get("output_tokens", 0) > 0 and seconds >= 0)


def main():
    ex, tr, rd = load("extract"), load("translate"), load("render")
    test_extract(ex)
    test_translate(tr)
    test_render_units(rd)
    test_render_e2e(rd)
    test_luna_e2e(tr)
    r = subprocess.run(["bash", str(HERE / "setup.sh"), "--check"], capture_output=True, text=True)
    check("setup.sh --check reports", "present:" in r.stdout, r.stdout + r.stderr)
    print(f"\n{len(fails)} failures" if fails else "\nall passed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
