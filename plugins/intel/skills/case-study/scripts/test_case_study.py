#!/usr/bin/env python3
# ABOUTME: Tests the case-study work-dir scaffold and the pre-render check.
# ABOUTME: Covers init (layout, idempotence) and check (lead paragraphs, sources, counts).

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
        meta = cs.init("jane-doe", "Jane Doe: how the account grew", "https://example.com/@jane", out, 12)
        work = Path(meta["work"])

        check("init puts the work dir under the store's .work", work == tmp / "store" / ".work" / "jane-doe", str(work))
        check("init creates md, raw and review", all((work / d).is_dir() for d in ("md", "raw", "review")))
        check("init writes an empty sources.json", json.loads((work / "sources.json").read_text()) == {})
        saved = json.loads((work / "chapters.json").read_text())
        check("chapters.json lists 12 chapters", [c["id"] for c in saved["chapters"]] == [f"{n:02d}" for n in range(1, 13)])
        check("chapters point at md/NN.md", saved["chapters"][2]["highlights"] == "md/03.md")
        check("chapters.json carries the title and pdf path", saved["title"].startswith("Jane Doe") and saved["pdf"] == str(out))
        check("chapters.json is a chapter-unit book with a page size", saved["unit"] == "chapter" and len(saved["page_size"]) == 2)

        (work / "sources.json").write_text(json.dumps({"https://a.example/x": "A 2020"}))
        cs.init("jane-doe", "Jane Doe: how the account grew", "https://example.com/@jane", out, 12)
        check("init again keeps an existing sources.json", json.loads((work / "sources.json").read_text()) == {"https://a.example/x": "A 2020"})

        report = cs.check(work)
        check("check reports every missing chapter", len(report["missing_chapters"]) == 12, str(report["missing_chapters"]))
        check("check fails while chapters are missing", report["ok"] is False)

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
        check("check fails on a chapter without a lead paragraph", report["ok"] is False)

        (work / "md" / "04.md").write_text("# Chapter 4\n\nLead.\n\n## Section\n\nBody.\n")
        report = cs.check(work)
        check("check passes a complete study", report["ok"] is True, str(report))

        (work / "sources.json").write_text("[1, 2]")
        report = cs.check(work)
        check("check rejects a sources.json that is not a url-to-label object", report["ok"] is False and report["sources_valid"] is False)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print(f"\n{len(fails)} failed" if fails else "\nall passed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
