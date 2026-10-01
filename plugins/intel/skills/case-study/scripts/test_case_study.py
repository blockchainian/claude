#!/usr/bin/env python3
# ABOUTME: Tests the case-study work-dir scaffold and the pre-render check.
# ABOUTME: Covers init (layout, cover, idempotence) and check (sourced draft, book text, sources, counts).

import importlib.util
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
fails = []


def check(name, ok, detail=""):
    print(("ok   " if ok else "FAIL ") + name + (f" — {detail}" if detail and not ok else ""))
    if not ok:
        fails.append(name)


def load():
    spec = importlib.util.spec_from_file_location("case_study", HERE / "case_study.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def main():
    tmp = Path(tempfile.mkdtemp())
    os.environ["HIGHLIGHTS_DIR"] = str(tmp / "store")
    cs = load()
    try:
        out = tmp / "pdf" / "jane-doe.pdf"
        meta = cs.init("jane-doe", "How Jane Doe grew", "https://example.com/@jane", out, 12, "Jane Doe")
        work = Path(meta["work"])

        check("init puts the work dir under the store's .work", work == tmp / "store" / ".work" / "jane-doe", str(work))
        check("init creates md, book, raw and review", all((work / d).is_dir() for d in ("md", "book", "raw", "review")))
        check("init writes an empty sources.json", json.loads((work / "sources.json").read_text()) == {})
        saved = json.loads((work / "chapters.json").read_text())
        check("chapters.json lists 12 chapters", [c["id"] for c in saved["chapters"]] == [f"{n:02d}" for n in range(1, 13)])
        check("the typeset chapters are the book text", saved["chapters"][2]["highlights"] == "book/03.md")
        check("chapters.json carries the title and pdf path", saved["title"] == "How Jane Doe grew" and saved["pdf"] == str(out))
        check("chapters.json carries the cover name", saved["cover"] == "Jane Doe")
        check("chapters.json is a chapter-unit book with a page size", saved["unit"] == "chapter" and len(saved["page_size"]) == 2)

        (work / "sources.json").write_text(json.dumps({"https://a.example/x": "A 2020"}))
        cs.init("jane-doe", "How Jane Doe grew", "https://example.com/@jane", out, 12, "Jane Doe")
        check("init again keeps an existing sources.json", json.loads((work / "sources.json").read_text()) == {"https://a.example/x": "A 2020"})

        report = cs.check(work)
        check("check reports every missing chapter", len(report["missing_chapters"]) == 12, str(report["missing_chapters"]))
        check("check fails while chapters are missing", report["draft_ok"] is False and report["ok"] is False)

        for n in range(1, 13):
            (work / "md" / f"{n:02d}.md").write_text(f"# Chapter {n}\n\nLead paragraph.\n\n## Section\n\nBody.\n")
        (work / "md" / "04.md").write_text("# Chapter 4\n\n## Section first\n\nBody.\n")
        (work / "sources.json").write_text(json.dumps({
            "https://www.a.example/x": "A 2020",
            "https://a.example/y": "A 2021",
            "https://b.example/z": "B 2019",
            "https://web.archive.org/web/20200101000000/https://c.example/p": "Archive 2020",
            "https://web.archive.org/web/20210101000000/https://c.example/p": "Archive 2021",
        }))
        report = cs.check(work)
        check("check finds the chapter without a lead paragraph", report["no_lead_paragraph"] == ["04"], str(report["no_lead_paragraph"]))
        check("check counts archive snapshots apart from other sources", report["sources"] == 3 and report["archive_snapshots"] == 2, str(report))
        check("check counts distinct sites without www and without the archive", report["sites"] == 2, str(report["sites"]))
        check("check fails on a chapter without a lead paragraph", report["draft_ok"] is False)

        (work / "md" / "04.md").write_text("# Chapter 4\n\nLead.\n\n## Section\n\nBody.\n")
        report = cs.check(work)
        check("check passes a complete sourced draft", report["draft_ok"] is True, str(report))

        check("check reports the book text as missing until it is written", len(report["missing_book_chapters"]) == 12 and report["book_ok"] is False, str(report))

        for n in range(1, 13):
            (work / "md" / f"{n:02d}.md").write_text(f"# Chapter {n}\n\nShe had 19,936 followers in 2016 (Outlet 2020).\n\n## Section\n\nBody.\n")
            (work / "book" / f"{n:02d}.md").write_text(f"# Chapter {n}\n\nShe had 19,936 followers in 2016.\n\n## Section\n\nBody.\n")
        report = cs.check(work)
        check("check passes clean book text", report["book_ok"] is True and report["ok"] is True, str(report))

        (work / "book" / "02.md").write_text("# Chapter 2\n\nShe had 19,936 followers (Outlet 2020).\n\n## Section\n\n过了 100 万（Tubefilter 2022（人物页））。\n")
        (work / "book" / "03.md").write_text("# Chapter 3\n\nIn 2016 (she was 19) it grew; the snapshot shows it, per sources.json.\n\n## Section\n\n没有第二个独立来源可以核对。\n")
        (work / "book" / "05.md").write_text("# Chapter 5\n\nShe had 21,000 followers in 2016.\n\n## Section\n\nBody.\n")
        (work / "book" / "12.md").write_text("# Sources\n\nThe list.\n\n## Press\n\n- Outlet 2020\n- Archive 2031\n")
        report = cs.check(work)
        check("check finds source citations left in the book text", [c["chapter"] for c in report["citations_in_book"]] == ["02"]
              and len(report["citations_in_book"][0]["found"]) == 2, str(report["citations_in_book"]))
        check("a plain parenthesis with an age or a date is not a citation", "03" not in [c["chapter"] for c in report["citations_in_book"]])
        check("check finds research-process wording in the book text", [c["chapter"] for c in report["process_terms_in_book"]] == ["03"]
              and {"snapshot", "sources.json", "独立来源", "核对"} <= set(report["process_terms_in_book"][0]["found"]), str(report["process_terms_in_book"]))
        check("check finds a figure the sourced draft does not have", {"chapter": "05", "found": ["21000"]} in report["numbers_not_in_draft"], str(report["numbers_not_in_draft"]))
        check("the closing sources chapter may list outlets and years", "12" not in [c["chapter"] for c in report["citations_in_book"] + report["numbers_not_in_draft"]]
              and "01" not in [c["chapter"] for c in report["numbers_not_in_draft"]])
        check("check fails on unclean book text", report["book_ok"] is False and report["ok"] is False)

        (work / "md" / "06.md").write_text("# Chapter 6\n\nShe had 24.8M subscribers and 1.2B views in 2016.\n\n## Section\n\nBody.\n")
        (work / "book" / "06.md").write_text("# Chapter 6\n\n2016 年她有 2,480 万订阅、12 亿播放。\n\n## Section\n\nBody.\n")
        report = cs.check(work)
        check("a figure restated in another unit of ten is the draft's figure", "06" not in [c["chapter"] for c in report["numbers_not_in_draft"]], str(report["numbers_not_in_draft"]))

        (work / "sources.json").write_text("[1, 2]")
        report = cs.check(work)
        check("check rejects a sources.json that is not a url-to-label object", report["draft_ok"] is False and report["sources_valid"] is False)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print(f"\n{len(fails)} failed" if fails else "\nall passed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
