#!/usr/bin/env python3
# ABOUTME: Scaffolds a case-study work dir in the digest store and checks it before rendering.
# ABOUTME: init writes chapters.json + sources.json; check verifies the sourced draft, the book text and sources.
#
# Usage: case_study.py init <slug> --title <title> --cover <name> --source <url> --out <pdf> [--chapters 12]
#        case_study.py merge <work dir>
#        case_study.py slice <work dir> <n> <of>      (the urls of one reviewer's slice, one per line)
#        case_study.py figures <work dir> <NN> [--worklist]   (match chapter NN's figures against the saved source text)
#        case_study.py findings <work dir> <NN>       (the review lines one fixer applies to chapter NN)
#        case_study.py check <work dir> [--draft]
# A study has two layers: md/NN.md is the sourced draft the reviewers audit (sources named in every sentence);
# book/NN.md is the text that is typeset (no citations, no account of the research).
# Agents working in parallel never share a file: each writes its own notes/<name>.sources.json (url -> label) and
# notes/<name>.gaps.md, reviewers write review/<name>.failed.json (a list of urls), fixers write
# review/<name>.added.json (url -> label). merge turns those into sources.json, gaps.md and the draft's last chapter,
# and notes/<name>.raw.json (url -> the files its text was saved to) into raw.json.
import argparse
import json
import os
import re
import sys
from pathlib import Path
from urllib.parse import urlparse

ROOT = Path(os.environ.get("HIGHLIGHTS_DIR", Path.home() / "Documents" / "highlights"))
PAGE_SIZE = [427.92, 660.0]  # the book format digest's render typesets
ARCHIVE_HOSTS = {"web.archive.org", "archive.org"}
# A parenthesis that names a source: a word, then a year that is not part of a date.
CITATION = re.compile(r"[（(][^（()）]*?(?P<word>[A-Za-z\u4e00-\u9fff][\w.&'’-]*)\s+(?:19|20)\d\d(?!\s*年)[^（()）]*[）)（(]")
DATE_WORDS = {"in", "since", "from", "by", "until", "to", "of", "late", "early", "mid", "born", "and", "january",
              "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
              "december"}
# Wording that describes the research instead of the subject.
PROCESS_TERMS = ["sources.json", "notes.md", "gaps.md", "subagent", "research agent", "调查 agent", "snapshot",
                 "yt-dlp", "self-reported", "independent source", "独立来源", "核对", "快照", "存档页", "有记录佐证",
                 "本人说的", "当时的报道", "事后报道"]
NUMBER = re.compile(r"\d[\d,]*(?:\.\d+)?")
# A figure with the unit that scales it; separators inside it may be commas, dots or spaces.
CJK_UNITS = ("十万", "百万", "千万", "万", "亿", "千")
UNITS = {"": 1, "k": 1e3, "K": 1e3, "thousand": 1e3, "m": 1e6, "M": 1e6, "mn": 1e6, "million": 1e6, "bn": 1e9, "b": 1e9, "B": 1e9,
         "billion": 1e9, "千": 1e3, "万": 1e4, "十万": 1e5, "百万": 1e6, "千万": 1e7, "亿": 1e8}
AMOUNT = re.compile(r"(?<![\w.])(\d{1,3}(?:[ \u00a0]\d{3})+(?!\d)|\d[\d,.]*\d|\d)\s*"
                    r"(十万|百万|千万|万|亿|千|thousand\b|million\b|billion\b|mn\b|bn\b|[kKmMbB]\b)?")
CHART = re.compile(r"^```chart[ \t]*\n.*?\n```[ \t]*$", re.S | re.M)  # a figure block the renderer draws
# A date, whose digits are not figures of a series: 2012 年 5 月 2 日, 8 月 2 日, 2012-05-02, a bare year.
DATE = re.compile(r"(?:19|20)\d\d\s*年(?:\s*\d+\s*月)?(?:\s*\d+\s*日)?|\d+\s*月(?:\s*\d+\s*日)?|\d+\s*日|(?:19|20)\d\d(?:-\d\d){0,2}")
SERIES = 5  # a paragraph with this many figures besides its dates recites a series


def init(slug, title, source, out, chapters, cover):
    """Create <store>/.work/<slug>/ with md/, book/, notes/, raw/, review/, chapters.json and an empty sources.json."""
    work = ROOT / ".work" / slug
    for sub in ("md", "book", "notes", "raw", "review"):
        (work / sub).mkdir(parents=True, exist_ok=True)
    ids = [f"{n:02d}" for n in range(1, chapters + 1)]
    meta = {"title": title, "cover": cover, "slug": slug, "source": source, "pdf": str(out), "work": str(work),
            "unit": "chapter", "page_size": PAGE_SIZE,
            "chapters": [{"id": i, "title": i, "pages": [1, 1], "chars": 0,
                          "text": f"text/{i}.txt", "highlights": f"book/{i}.md"} for i in ids]}
    (work / "chapters.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2), encoding="utf-8")
    sources = work / "sources.json"
    if not sources.exists():
        sources.write_text("{}", encoding="utf-8")
    return meta


def read_json(path, kind):
    """The JSON in a file when it is of the expected kind, else an empty one."""
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return kind()
    return data if isinstance(data, kind) else kind()


def merge(work):
    """Rebuild sources.json, gaps.md and the draft's sources chapter from the files the parallel agents wrote."""
    work = Path(work)
    meta = json.loads((work / "chapters.json").read_text(encoding="utf-8"))
    sources = {}
    for path in sorted((work / "notes").glob("*.sources.json")) + sorted((work / "review").glob("*.added.json")):
        sources.update({k: v for k, v in read_json(path, dict).items() if isinstance(v, str)})
    failed = {url for path in sorted((work / "review").glob("*.failed.json")) for url in read_json(path, list)}
    sources = {url: label for url, label in sources.items() if url not in failed}
    (work / "sources.json").write_text(json.dumps(sources, ensure_ascii=False, indent=2), encoding="utf-8")
    saved = {}
    for path in sorted((work / "notes").glob("*.raw.json")) + sorted((work / "review").glob("*.raw.json")):
        for url, files in read_json(path, dict).items():
            saved.setdefault(url, []).extend(f for f in ([files] if isinstance(files, str) else files) if f not in saved[url])
    (work / "raw.json").write_text(json.dumps(saved, ensure_ascii=False, indent=2), encoding="utf-8")
    gaps = [path.read_text(encoding="utf-8").strip() for path in sorted((work / "notes").glob("*.gaps.md"))]
    (work / "gaps.md").write_text("\n".join(g for g in gaps if g) + "\n", encoding="utf-8")
    labels = sorted({f"{label} ({urlparse(url).netloc.removeprefix('www.')})" for url, label in sources.items()})
    last = work / "md" / f"{meta['chapters'][-1]['id']}.md"
    last.write_text(f"# Sources\n\n{len(sources)} sources, listed in sources.json.\n\n## List\n\n"
                    + "\n".join(f"- {label}" for label in labels) + "\n", encoding="utf-8")
    return {"sources": len(sources), "failed": len(failed), "gaps": sum(g.count("\n") + 1 for g in gaps if g)}


def slice_sources(work, index, of):
    """Slice number index (from 1) of the source urls, sorted, cut into `of` near-equal parts."""
    urls = sorted(read_json(Path(work) / "sources.json", dict))
    size = -(-len(urls) // of)
    return urls[(index - 1) * size:index * size]


def findings(work, chapter):
    """The review lines a fixer applies to one chapter: those tagged with its number, and the sources-lens lines
    (tagged with a source label) about labels the chapter names."""
    work = Path(work)
    text = (work / "md" / f"{chapter}.md").read_text(encoding="utf-8")
    out = []
    for path in sorted((work / "review").glob("*.md")):
        for line in path.read_text(encoding="utf-8").splitlines():
            tag = re.match(r"- \[([^\]]+)\]", line)
            if tag and (tag.group(1) == chapter or (path.name.startswith("sources-") and tag.group(1) in text)):
                out.append(line)
    return out


def amounts(text):
    """Every figure in a text as (the figure as written, the values it may stand for, its digits without leading
    or trailing zeros). A unit after the figure scales the value: 1,200 万 and 12 million are both 12,000,000.
    One separator before a last group of three digits may be a thousands mark or a decimal point: both are kept."""
    found = []
    for m in AMOUNT.finditer(text):
        token, unit = m.group(1), m.group(2) or ""
        groups = re.split(r"[.,\s\u00a0]", token)
        whole = float("".join(groups))
        decimal = float("".join(groups[:-1]) + "." + groups[-1]) if len(groups) > 1 else whole
        if len(groups) == 1 or (len(groups) > 2 and len(groups[-1]) == 3):
            numbers = [whole]
        elif len(groups[-1]) == 3:
            numbers = [whole, decimal]
        else:
            numbers = [decimal]
        scale = UNITS[unit if unit in UNITS else unit.lower()]
        written = f"{token} {unit}".strip() if unit in CJK_UNITS else token + unit
        found.append((written, [round(n * scale, 6) for n in numbers], "".join(groups).strip("0")))
    return found


def figures(work, chapter, worklist=False):
    """Match every figure in a draft chapter against the saved text of the sources its sentence names, and write
    the ones that are not there as findings for the chapter's fixer, or, as a worklist, for a reviewer to judge."""
    work = Path(work)
    labels = {}
    for url, label in read_json(work / "sources.json", dict).items():
        labels.setdefault(label, []).append(url)
    saved = read_json(work / "raw.json", dict)
    texts = {}

    def known(label):
        """(values, digit strings) of the figures in everything saved for a label's sources."""
        if label not in texts:
            parts = [(work / f).read_text(encoding="utf-8", errors="ignore") for url in labels[label] for f in saved.get(url, [])
                     if (work / f).is_file()]
            found = amounts("\n".join(parts))
            texts[label] = ({v for _, values, _ in found for v in values}, {d for _, _, d in found if len(d) >= 4})
        return texts[label]

    ordered = sorted(labels, key=len, reverse=True)
    named = lambda text: [label for label in ordered if label in text]
    checked = small = 0
    unmatched, lines = [], []
    body = (work / "md" / f"{chapter}.md").read_text(encoding="utf-8")
    anywhere = named(body)  # a paragraph that names no source (a lead, a summary) restates the chapter's own figures
    for paragraph in re.split(r"\n\s*\n", body):
        if paragraph.lstrip().startswith("#"):
            continue
        for sentence in re.split(r"(?<=[。！？!?])|(?<=\.)\s+(?=[A-Z])|\n", paragraph):
            cited = named(sentence) or named(paragraph) or anywhere
            bare = sentence
            for label in ordered:
                bare = bare.replace(label, "")
            missing = []
            for written, values, digits in amounts(DATE.sub("", bare)):
                if max(values) < 100 and len(digits) < 3:
                    small += 1
                    continue
                checked += 1
                if not any(set(values) & known(label)[0] or (len(digits) >= 4 and digits in known(label)[1]) for label in cited):
                    missing.append(written)
                    unmatched.append({"figure": written, "labels": cited, "sentence": sentence.strip()})
            if missing:
                where = f"not in the saved text of {', '.join(cited)}" if cited else "its sentence names no source"
                lines.append(f"{'*' if worklist else f'- [{chapter}] unsupported |'} {sentence.strip()[:200]} | {', '.join(missing)}: {where} | | "
                             "open the source and find each figure: correct it, say what it was computed from if it is derived, or remove it")
    (work / "review").mkdir(exist_ok=True)
    (work / "review" / f"figures-{chapter}.md").write_text(
        f"# Figures in md/{chapter}.md matched against the saved text of their sources\n\n"
        f"{checked} checked, {checked - len(unmatched)} matched, {len(unmatched)} not matched, {small} too small to match (left to the quotes lens).\n\n"
        + "\n".join(lines) + "\n", encoding="utf-8")
    return {"chapter": chapter, "checked": checked, "matched": checked - len(unmatched), "small": small, "unmatched": unmatched}


def has_lead_paragraph(text):
    """True when prose sits between the chapter's title line and its first '## ' heading."""
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    body = lines[1:] if lines and lines[0].startswith("# ") else lines
    return bool(body) and not body[0].startswith("#")


def numbers(text):
    """The figures in a text, without thousands separators."""
    return {m.group().replace(",", "").rstrip(".") for m in NUMBER.finditer(text)}


def recited_series(text):
    """The openings of the paragraphs that recite a run of figures in prose instead of showing a chart or a table."""
    found = []
    for paragraph in re.split(r"\n\s*\n", CHART.sub("", text)):
        if not paragraph.lstrip().startswith("#") and len(NUMBER.findall(DATE.sub("", paragraph))) >= SERIES:
            found.append(paragraph.strip()[:40])
    return found


def restated(number, known_values):
    """True when the figure is a known one in another unit: 24.8M as 2,480 万, 1.2B as 12 亿."""
    value = float(number)
    return any(abs(value - k * 10 ** e) <= 1e-9 * max(value, 1) for k in known_values for e in range(-4, 5))


def layer(work, folder, ids):
    """(missing ids, ids with no lead paragraph, id -> text) for one layer of chapters."""
    missing, no_lead, texts = [], [], {}
    for i in ids:
        path = work / folder / f"{i}.md"
        text = path.read_text(encoding="utf-8") if path.exists() else ""
        if not text.strip():
            missing.append(i)
            continue
        texts[i] = text
        if not has_lead_paragraph(text):
            no_lead.append(i)
    return missing, no_lead, texts


def check(work):
    """Report what blocks the review (the sourced draft, sources.json) and what blocks rendering (the book text)."""
    work = Path(work)
    meta = json.loads((work / "chapters.json").read_text(encoding="utf-8"))
    ids = [chapter["id"] for chapter in meta["chapters"]]
    missing, no_lead, draft = layer(work, "md", ids)
    book_missing, book_no_lead, book = layer(work, "book", ids)
    known = set().union(*(numbers(text) for text in draft.values())) if draft else set()
    known_values = [float(n) for n in known]
    citations, process, unknown, series = [], [], [], []
    for i, text in book.items():
        terms = [term for term in PROCESS_TERMS if term in text.lower()]
        if terms and i != ids[-1]:
            process.append({"chapter": i, "found": terms})
        if i == ids[-1]:  # the closing sources chapter lists outlets and years
            continue
        cited = [m.group() for m in CITATION.finditer(text) if m.group("word").lower() not in DATE_WORDS]
        if cited:
            citations.append({"chapter": i, "found": cited})
        extra = sorted(n for n in numbers(text) - known if not restated(n, known_values))
        if extra:
            unknown.append({"chapter": i, "found": extra})
        recited = recited_series(text)
        if recited:
            series.append({"chapter": i, "found": recited})
    try:
        sources = json.loads((work / "sources.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        sources = None
    valid = isinstance(sources, dict) and all(isinstance(k, str) and isinstance(v, str) for k, v in sources.items())
    urls = list(sources) if isinstance(sources, dict) and valid else []
    hosts = [urlparse(url).netloc.removeprefix("www.") for url in urls]
    snapshots = sum(host in ARCHIVE_HOSTS for host in hosts)
    draft_ok = valid and not missing and not no_lead
    book_ok = not (book_missing or book_no_lead or citations or process or unknown or series)
    return {"ok": draft_ok and book_ok, "draft_ok": draft_ok, "book_ok": book_ok,
            "missing_chapters": missing, "no_lead_paragraph": no_lead, "sources_valid": valid,
            "missing_book_chapters": book_missing, "book_no_lead_paragraph": book_no_lead,
            "citations_in_book": citations, "process_terms_in_book": process, "numbers_not_in_draft": unknown,
            "series_in_prose": series,
            "sources": len(hosts) - snapshots, "archive_snapshots": snapshots,
            "sites": len({host for host in hosts if host not in ARCHIVE_HOSTS})}


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("init")
    p.add_argument("slug")
    p.add_argument("--title", required=True)
    p.add_argument("--cover", required=True, help="the subject's name, set large on the cover")
    p.add_argument("--source", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--chapters", type=int, default=12)
    p = sub.add_parser("merge")
    p.add_argument("work")
    p = sub.add_parser("slice")
    p.add_argument("work")
    p.add_argument("index", type=int)
    p.add_argument("of", type=int)
    p = sub.add_parser("figures")
    p.add_argument("work")
    p.add_argument("chapter", help="the chapter number, e.g. 04")
    p.add_argument("--worklist", action="store_true", help="list the unmatched figures for a reviewer instead of as findings")
    p = sub.add_parser("findings")
    p.add_argument("work")
    p.add_argument("chapter", help="the chapter number, e.g. 04")
    p = sub.add_parser("check")
    p.add_argument("work")
    p.add_argument("--draft", action="store_true", help="pass on the sourced draft alone, before the book text exists")
    args = parser.parse_args()
    if args.cmd == "slice":
        print("\n".join(slice_sources(args.work, args.index, args.of)))
        return
    if args.cmd == "figures":
        result = figures(args.work, args.chapter, args.worklist)
        print(json.dumps({k: (len(v) if k == "unmatched" else v) for k, v in result.items()}))
        return
    if args.cmd == "findings":
        print("\n".join(findings(args.work, args.chapter)))
        return
    if args.cmd == "init":
        result = init(args.slug, args.title, args.source, Path(args.out).expanduser(), args.chapters, args.cover)
        passed = True
    elif args.cmd == "merge":
        result = merge(args.work)
        passed = True
    else:
        result = check(args.work)
        passed = result["draft_ok"] if args.draft else result["ok"]
    print(json.dumps(result, ensure_ascii=False, indent=2))
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
