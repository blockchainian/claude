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
        check("init creates md, book, notes, raw and review", all((work / d).is_dir() for d in ("md", "book", "notes", "raw", "review")))
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

        series = "2012 年 5 月 2 日是 603 个，8 月 2 日 762 个，11 月 2 日 969 个，2013 年 2 月 1 日 1,128 个，每月约 52 到 68 个。"
        (work / "md" / "07.md").write_text(f"# Chapter 7\n\nLead.\n\n## Section\n\n{series}\n")
        (work / "book" / "07.md").write_text(f"# Chapter 7\n\nLead.\n\n## Section\n\n{series}\n")
        report = cs.check(work)
        check("a paragraph that recites a series of figures is reported: it belongs in a chart", [c["chapter"] for c in report["series_in_prose"]] == ["07"]
              and report["series_in_prose"][0]["found"][0].startswith("2012 年 5 月 2 日是 603") and report["book_ok"] is False, str(report["series_in_prose"]))
        chart = "```chart\ntype: line\ntitle: Videos\n2012-05-02 | 603\n2012-08-02 | 762\n2012-11-02 | 969\n2013-02-01 | 1,128\n```"
        (work / "book" / "07.md").write_text(f"# Chapter 7\n\nLead.\n\n## Section\n\n每月约 52 到 68 个，2013 年 2 月 1 日到 1,128 个。\n\n{chart}\n")
        report = cs.check(work)
        check("the same figures in a chart block pass, and are still checked against the draft", report["series_in_prose"] == []
              and "07" not in [c["chapter"] for c in report["numbers_not_in_draft"]], str(report))
        (work / "book" / "07.md").write_text(f"# Chapter 7\n\nLead.\n\n{chart.replace('1,128', '1,182')}\n")
        report = cs.check(work)
        check("a chart value the draft does not have is reported", {"chapter": "07", "found": ["1182"]} in report["numbers_not_in_draft"], str(report["numbers_not_in_draft"]))
        (work / "book" / "07.md").write_text("# Chapter 7\n\nLead.\n\n## Section\n\nBody.\n")

        (work / "notes" / "read-01.sources.json").write_text(json.dumps({"https://a.example/x": "A 2020", "https://seller.example/s": "Seller 2021"}))
        (work / "notes" / "read-02.sources.json").write_text(json.dumps({"https://b.example/z": "B 2019"}))
        (work / "notes" / "read-01.gaps.md").write_text("- could not reach c.example\n")
        (work / "notes" / "read-02.gaps.md").write_text("- paywall at d.example\n")
        merged = cs.merge(work)
        saved = json.loads((work / "sources.json").read_text())
        check("merge builds sources.json from every agent's source list", set(saved) == {"https://a.example/x", "https://seller.example/s", "https://b.example/z"} and merged["sources"] == 3, str(saved))
        check("merge gathers the gaps", "c.example" in (work / "gaps.md").read_text() and "d.example" in (work / "gaps.md").read_text())
        (work / "review" / "sources-1.failed.json").write_text(json.dumps(["https://seller.example/s"]))
        (work / "review" / "fix-03.added.json").write_text(json.dumps({"https://e.example/new": "E 2022"}))
        cs.merge(work)
        saved = json.loads((work / "sources.json").read_text())
        check("merge drops sources a reviewer failed and adds what the fix round read", set(saved) == {"https://a.example/x", "https://b.example/z", "https://e.example/new"}, str(saved))
        parts = [cs.slice_sources(work, n, 2) for n in (1, 2)]
        check("the sources split into slices that cover every url once", sorted(parts[0] + parts[1]) == sorted(saved) and len(parts[0]) == 2 and len(parts[1]) == 1, str(parts))
        (work / "review" / "numbers-3.md").write_text("| row | checked |\n- [03] wrong | 24.8M | source says 24.6M | 24.6M | print 24.6M\n- [04] missing | a | b | c | d\n")
        (work / "review" / "sources-1.md").write_text("- [A 2020] seller-source | sells a course | | drop\n- [B 2019] mislabelled | a | b | c | d\n")
        (work / "md" / "03.md").write_text("# Chapter 2\n\nShe had 24.8M subscribers (on record, A 2020).\n\n## Section\n\nBody.\n")
        found = cs.findings(work, "03")
        check("findings for a chapter are its own lines plus the sources-lens lines about labels the chapter names",
              found == ["- [03] wrong | 24.8M | source says 24.6M | 24.6M | print 24.6M", "- [A 2020] seller-source | sells a course | | drop"], str(found))
        (work / "notes" / "read-01.raw.json").write_text(json.dumps({"https://a.example/x": ["raw/a.html"], "https://b.example/z": ["raw/b1.txt"]}))
        (work / "notes" / "numbers-archive.raw.json").write_text(json.dumps({"https://b.example/z": ["raw/b2.txt"]}))
        cs.merge(work)
        saved_raw = json.loads((work / "raw.json").read_text())
        check("merge gathers where each source's text was saved", {u: sorted(f) for u, f in saved_raw.items()} == {"https://a.example/x": ["raw/a.html"], "https://b.example/z": ["raw/b1.txt", "raw/b2.txt"]}, str(saved_raw))
        (work / "raw" / "a.html").write_text('<p>She passed 1.002.877 subscribers and earned $12 million; <span title="24,8 miljoner">many</span> watched 603 videos.</p>')
        (work / "raw" / "b1.txt").write_text("The channel showed 29 321 179 subscribers, 11.941 million by another count.\n")
        (work / "raw" / "b2.txt").write_text("Views that week: 50,566,204. Turnover 7 million kronor.\n")
        (work / "md" / "05.md").write_text(
            "# Chapter 4\n\n她一共有 1,002,877 订阅，另一处写 11.941M。\n\n## Section\n\n"
            "2012 年 7 月 11 日她过了 1,002,877 订阅，收入 1,200 万美元，有 603 条视频（A 2020，当时的报道）。"
            "B 2019 的页面写 29,321,179 订阅、当周 50,566,204 次观看。"
            "她 7 天发了 8 条，占 45%（A 2020）。\n\n"
            "她的公司营业额 720 万克朗，另一篇写 2,480 万人看过（B 2019）。"
            "同一年她有 31,000 个付费会员。\n\n"
            "E 2022 写她有 5,555 个订阅。\n")
        report = cs.figures(work, "05")
        missed = {(m["figure"], tuple(m["labels"])) for m in report["unmatched"]}
        check("figures the cited source's saved text holds are matched: other separators, a unit for the same value, a spaced number",
              not {f for f, _ in missed} & {"1,002,877", "1,200 万", "603", "29,321,179", "50,566,204"}, str(missed))
        check("a figure that is in another source's text, or in none, is reported with the sources its sentence names",
              ("720 万", ("B 2019",)) in missed and ("2,480 万", ("B 2019",)) in missed, str(missed))
        check("a sentence that names no source takes its paragraph's", ("31,000", ("B 2019",)) in missed, str(missed))
        check("a source with no saved text leaves its figures unmatched", ("5,555", ("E 2022",)) in missed, str(missed))
        check("a sentence in a paragraph that names no source is matched against every source the chapter names, and a figure "
              "with three decimals is not read as thousands", not {f for f, _ in missed} & {"1,002,877", "11.941M"}, str(missed))
        check("dates and small figures are not checked", len(missed) == 4 and report["checked"] == 11 and report["small"] == 3, str(report))
        lines = (work / "review" / "figures-05.md").read_text().splitlines()
        check("the unmatched figures are written as findings for the chapter's fixer, one line per sentence",
              len([l for l in lines if l.startswith("- [05] unsupported | ")]) == 3 and any("720 万" in l and "2,480 万" in l and "B 2019" in l for l in lines), "\n".join(lines))
        check("the fixer receives them with its other findings", any("5,555" in l for l in cs.findings(work, "05")))
        cs.figures(work, "05", worklist=True)
        lines = (work / "review" / "figures-05.md").read_text().splitlines()
        check("as a reviewer's worklist the unmatched figures are not findings yet", len([l for l in lines if l.startswith("* ")]) == 3
              and not any("5,555" in l for l in cs.findings(work, "05")), "\n".join(lines))
        cs.figures(work, "05")
        last = (work / "md" / "12.md").read_text()
        check("merge writes the draft's sources chapter from sources.json", last.startswith("# Sources\n") and "A 2020" in last and "Seller 2021" not in last and cs.has_lead_paragraph(last), last[:200])

        (work / "sources.json").write_text("[1, 2]")
        report = cs.check(work)
        check("check rejects a sources.json that is not a url-to-label object", report["draft_ok"] is False and report["sources_valid"] is False)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print(f"\n{len(fails)} failed" if fails else "\nall passed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
