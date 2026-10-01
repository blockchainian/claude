#!/usr/bin/env python3
# ABOUTME: Scaffolds a case-study work dir in the digest store and checks it before rendering.
# ABOUTME: init writes chapters.json + sources.json; check verifies chapters, lead paragraphs and sources.
#
# Usage: case_study.py init <slug> --title <title> --source <url> --out <pdf> [--chapters 12]
#        case_study.py check <work dir>
import argparse
import json
import os
import sys
from pathlib import Path
from urllib.parse import urlparse

ROOT = Path(os.environ.get("HIGHLIGHTS_DIR", Path.home() / "Documents" / "highlights"))
PAGE_SIZE = [427.92, 660.0]  # the book format digest's render typesets
ARCHIVE_HOSTS = {"web.archive.org", "archive.org"}


def init(slug, title, source, out, chapters):
    """Create <store>/.work/<slug>/ with md/, raw/, review/, chapters.json and an empty sources.json."""
    work = ROOT / ".work" / slug
    for sub in ("md", "raw", "review"):
        (work / sub).mkdir(parents=True, exist_ok=True)
    ids = [f"{n:02d}" for n in range(1, chapters + 1)]
    meta = {"title": title, "slug": slug, "source": source, "pdf": str(out), "work": str(work),
            "unit": "chapter", "page_size": PAGE_SIZE,
            "chapters": [{"id": i, "title": i, "pages": [1, 1], "chars": 0,
                          "text": f"text/{i}.txt", "highlights": f"md/{i}.md"} for i in ids]}
    (work / "chapters.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2), encoding="utf-8")
    sources = work / "sources.json"
    if not sources.exists():
        sources.write_text("{}", encoding="utf-8")
    return meta


def has_lead_paragraph(text):
    """True when prose sits between the chapter's title line and its first '## ' heading."""
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    body = lines[1:] if lines and lines[0].startswith("# ") else lines
    return bool(body) and not body[0].startswith("#")


def check(work):
    """Report what blocks rendering: missing chapters, chapters with no lead paragraph, a malformed sources.json."""
    work = Path(work)
    meta = json.loads((work / "chapters.json").read_text(encoding="utf-8"))
    missing, no_lead = [], []
    for chapter in meta["chapters"]:
        path = work / chapter["highlights"]
        if not path.exists() or not path.read_text(encoding="utf-8").strip():
            missing.append(chapter["id"])
        elif not has_lead_paragraph(path.read_text(encoding="utf-8")):
            no_lead.append(chapter["id"])
    try:
        sources = json.loads((work / "sources.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        sources = None
    valid = isinstance(sources, dict) and all(isinstance(k, str) and isinstance(v, str) for k, v in sources.items())
    urls = list(sources) if isinstance(sources, dict) and valid else []
    hosts = [urlparse(url).netloc.removeprefix("www.") for url in urls]
    snapshots = sum(host in ARCHIVE_HOSTS for host in hosts)
    return {"ok": valid and not missing and not no_lead,
            "missing_chapters": missing, "no_lead_paragraph": no_lead, "sources_valid": valid,
            "sources": len(hosts) - snapshots, "archive_snapshots": snapshots,
            "sites": len({host for host in hosts if host not in ARCHIVE_HOSTS})}


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("init")
    p.add_argument("slug")
    p.add_argument("--title", required=True)
    p.add_argument("--source", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--chapters", type=int, default=12)
    p = sub.add_parser("check")
    p.add_argument("work")
    args = parser.parse_args()
    if args.cmd == "init":
        result = init(args.slug, args.title, args.source, Path(args.out).expanduser(), args.chapters)
    else:
        result = check(args.work)
    print(json.dumps(result, ensure_ascii=False, indent=2))
    sys.exit(0 if result.get("ok", True) else 1)


if __name__ == "__main__":
    main()
