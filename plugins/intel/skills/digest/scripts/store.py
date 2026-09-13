#!/usr/bin/env python3
# ABOUTME: Central store for source highlights - save, list and search.
# ABOUTME: One markdown file per item under episodes/, rebuilt index.md.

import os
import re
import sys
from datetime import date
from pathlib import Path

ROOT = Path(os.environ.get("PODCAST_HIGHLIGHTS_DIR",
                           Path.home() / ".claude" / "podcast-highlights"))
EPISODES = ROOT / "episodes"
INDEX = ROOT / "index.md"


def parse_frontmatter(text):
    if not text.startswith("---\n"):
        return {}, text
    end = text.find("\n---\n", 4)
    if end == -1:
        return {}, text
    meta = {}
    for line in text[4:end].splitlines():
        if ":" in line and not line.startswith(" "):
            k, v = line.split(":", 1)
            meta[k.strip()] = v.strip().strip('"')
    return meta, text[end + 5:]


def rebuild_index():
    rows = []
    for f in sorted(EPISODES.glob("*.md")):
        meta, _ = parse_frontmatter(f.read_text(encoding="utf-8"))
        rows.append((meta.get("saved", ""), meta.get("source") or meta.get("show") or "?",
                     meta.get("title", f.stem), f.name, meta.get("url", "")))
    rows.sort(reverse=True)
    lines = ["# Highlights", ""]
    lines += [f"- {saved} — **{show}** — [{title}](episodes/{name}) — {url}"
              for saved, show, title, name, url in rows]
    INDEX.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return len(rows)


def free_slot(slug, url):
    """The file for this episode: its own, or the next free name beside it."""
    base, n = EPISODES / f"{slug}.md", 1
    dest = base
    while dest.exists():
        meta, _ = parse_frontmatter(dest.read_text(encoding="utf-8"))
        if meta.get("url", "") == url:
            return dest, True
        n += 1
        dest = EPISODES / f"{slug}-{n}.md"
    return dest, False


def cmd_save(draft):
    src = Path(draft)
    if not src.is_file():
        raise SystemExit(f"no such draft: {src}")
    text = src.read_text(encoding="utf-8")
    meta, _ = parse_frontmatter(text)
    missing = [k for k in ("title", "url") if not meta.get(k)]
    if missing:
        raise SystemExit("draft frontmatter is missing: " + ", ".join(missing))
    if not meta.get("saved"):
        text = text.replace("---\n", f"---\nsaved: {date.today()}\n", 1)
        meta["saved"] = str(date.today())
    slug = meta.get("slug") or (src.stem if src.parent == EPISODES
                                else src.parent.name)
    slug = re.sub(r"[^a-z0-9]+", "-", slug.lower()).strip("-") or "item"
    EPISODES.mkdir(parents=True, exist_ok=True)
    dest, existed = free_slot(slug, meta["url"])
    if existed:
        print(f"already saved: {dest}")
        print('(no-op; add take-aways with: store.py takeaway <file> --add "...")')
        return
    dest.write_text(text, encoding="utf-8")
    total = rebuild_index()
    print(f"saved: {dest}")
    print(f"index: {INDEX} ({total} items)")


TAKEAWAYS_HEADING = "## Take-aways"


def read_takeaways(lines):
    """(heading_idx, [item line indices], end_idx) or (None, [], None)."""
    for i, line in enumerate(lines):
        if line.strip().lower() == TAKEAWAYS_HEADING.lower():
            j, items = i + 1, []
            while j < len(lines) and not lines[j].startswith("## "):
                if lines[j].lstrip().startswith("- "):
                    items.append(j)
                j += 1
            return i, items, j
    return None, [], None


def render_takeaways(items):
    return [TAKEAWAYS_HEADING, ""] + [f"- {it}" for it in items] + [""]


def first_section_idx(lines):
    for i, line in enumerate(lines):
        if line.startswith("## "):
            return i
    return None


def write_takeaways(dest, items):
    lines = dest.read_text(encoding="utf-8").split("\n")
    start, _, end = read_takeaways(lines)
    block = render_takeaways(items)
    if start is not None:
        lines[start:end] = block
    else:
        at = first_section_idx(lines)
        if at is None:
            lines = lines + ([""] if lines and lines[-1].strip() else []) + block
        else:
            lines[at:at] = block
    text = "\n".join(lines)
    dest.write_text(text.rstrip("\n") + "\n", encoding="utf-8")


def current_takeaways(dest):
    lines = dest.read_text(encoding="utf-8").split("\n")
    _, items, _ = read_takeaways(lines)
    return [lines[i].lstrip()[2:].strip() for i in items]


def print_takeaways(items):
    if not items:
        print("(no take-aways yet)")
        return
    for n, it in enumerate(items, 1):
        print(f"{n}. {it}")


def cmd_takeaway(target, action, index, text):
    dest = Path(target)
    if dest.parent != EPISODES or not dest.is_file():
        raise SystemExit(
            f"not a stored item: {dest}\n"
            "save the item first; take-aways attach to the stored file.")
    items = current_takeaways(dest)
    if action == "list":
        print_takeaways(items)
        return
    if action == "add":
        items.append(text)
    elif action == "revise":
        if index is None or not (1 <= index <= len(items)):
            raise SystemExit(f"--revise needs an item number in 1..{len(items)}")
        items[index - 1] = text
    write_takeaways(dest, items)
    print(f"take-aways for {dest.name}:")
    print_takeaways(current_takeaways(dest))


def cmd_search(query):
    if not EPISODES.is_dir():
        raise SystemExit(f"nothing saved yet ({EPISODES} does not exist)")
    try:
        pattern = re.compile(query, re.IGNORECASE)
    except re.error as exc:
        print(f"not a valid regex ({exc}) - searching for it literally")
        pattern = re.compile(re.escape(query), re.IGNORECASE)
    hits = 0
    for f in sorted(EPISODES.glob("*.md")):
        text = f.read_text(encoding="utf-8")
        meta, _ = parse_frontmatter(text)
        matches = [(n, l.strip()) for n, l in enumerate(text.splitlines(), 1)
                   if pattern.search(l)]
        if not matches:
            continue
        hits += 1
        print(f"\n=== {meta.get('source') or meta.get('show') or '?'} — {meta.get('title', f.stem)}")
        print(f"    {f}")
        if meta.get("url"):
            print(f"    {meta['url']}")
        for n, line in matches[:12]:
            print(f"    {n}: {line[:220]}")
        if len(matches) > 12:
            print(f"    ... {len(matches) - 12} more matches")
    print(f"\n{hits} item(s) matched {query!r}")


def cmd_list():
    if INDEX.is_file():
        print(INDEX.read_text(encoding="utf-8"))
    else:
        print(f"nothing saved yet ({INDEX} does not exist)")


USAGE = ("usage: store.py save <draft.md> | search <query> | list\n"
         "       store.py takeaway <stored.md> --list\n"
         "       store.py takeaway <stored.md> --add \"<text>\"\n"
         "       store.py takeaway <stored.md> --revise <n> \"<text>\"")


def parse_takeaway_args(args):
    if not args:
        raise SystemExit(USAGE)
    target, rest = args[0], args[1:]
    if rest[:1] == ["--list"]:
        return target, "list", None, None
    if rest[:1] == ["--add"] and len(rest) >= 2:
        return target, "add", None, " ".join(rest[1:])
    if rest[:1] == ["--revise"] and len(rest) >= 3 and rest[1].isdigit():
        return target, "revise", int(rest[1]), " ".join(rest[2:])
    raise SystemExit(USAGE)


def main():
    if len(sys.argv) < 2:
        raise SystemExit(USAGE)
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "save" and args:
        cmd_save(args[0])
    elif cmd == "takeaway":
        cmd_takeaway(*parse_takeaway_args(args))
    elif cmd == "search" and args:
        cmd_search(" ".join(args))
    elif cmd == "list":
        cmd_list()
    else:
        raise SystemExit(USAGE)


if __name__ == "__main__":
    main()
