#!/usr/bin/env python3
# ABOUTME: Translates each extracted section into Chinese Markdown with one no-tool codex exec call per section
# ABOUTME: (gpt-6-luna by default), many sections in flight at once, in a private CODEX_HOME; resumable.
#
# Usage: translate.py <work dir> [--only 04,05] [--jobs 20] [--model gpt-6-luna] [--effort low]
#                     [--service-tier priority] [--glossary <file>] [--force] [--dry-run]
# Reads <work>/sections.json; writes <work>/md/<id>-<slug>.md (+ .events.jsonl); skips sections whose .md exists.
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

RULES = """You are a professional literary translator (English -> Simplified Chinese) working on a published
non-fiction book. Everything you need is in the message: do not run commands, do not read or write files.
Translate the whole section, faithfully and fluently, as a Chinese publisher would print it. Never summarize,
never skip a paragraph, never add commentary.

The source is OCR text from a scanned book. Repair it silently: obvious misreads ("Pretace" -> "Preface",
"rnodern" -> "modern", "mineteen" -> "nineteen"), missing spaces in italics ("ofcustomer"), a first
letter that a drop cap detached or lost ("n 2006" -> "In 2006", "Ria we get started" -> "Before we get
started"), and duplicated or stray running heads.

Output Markdown only:
- First line: "# " + the section title in Chinese (no chapter number: the layout adds it).
- Sub-headings in the source (short title lines that are not sentences) become "## " lines.
- One paragraph per source paragraph, blank line between paragraphs. Keep "> " block quotes as "> ".
- Keep *emphasis*. Keep bullet lists as "- " lines.
- Proper nouns: company and product names stay in English (Google, Netflix, Y Combinator); people are
  given as 中文译名（English Name）the first time, then the Chinese name.
- Book and publication titles: 《中文译名》(English Title) the first time.
- Numbers, money and units stay as in the source (500 平方英尺, 2010 年秋天). Put one space between
  Chinese and Latin letters or digits.
- Follow the glossary exactly when one is given.
"""


def load_sections(work):
    meta = json.loads((work / "sections.json").read_text())
    return meta, [s for s in meta["sections"] if s.get("file")]


def build_prompt(meta, section, text, glossary=None):
    label = f"{section['label']} " if section.get("label") else ""
    head = [f"Book: {meta['title']}" + (f" by {meta['author']}" if meta.get("author") else ""),
            f"Section: {label}{section['title']} ({section['kind']}, {section.get('words', '?')} words)"]
    if glossary:
        head += ["Glossary (use these renderings):", glossary.strip()]
    return "\n".join(head) + "\n\nSource text:\n\n" + text.strip() + "\n"


def cjk_count(s):
    return len(re.findall(r"[一-鿿]", s))


def check_output(md, words):
    """A translation must start with the title heading and be long enough to have covered the source."""
    if not md.strip().startswith("# "):
        return "does not start with '# title'"
    if words and cjk_count(md) < 0.9 * words:
        return f"too short: {cjk_count(md)} Chinese characters for {words} English words"
    return None


OFF = ["shell_tool", "unified_exec", "unified_exec_tty", "view_image", "sleep_tool", "tool_suggest", "multi_agent",
       "plugins", "apps", "skill_search", "memories", "goals", "image_generation", "browser_use", "computer_use", "hooks"]


def codex_home(dir_, model, effort, instructions, service_tier):
    """A private CODEX_HOME: the login copied from ~/.codex, no user config, AGENTS.md, plugins or hooks."""
    home = dir_ / "home"
    home.mkdir(parents=True, exist_ok=True)
    src = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex")) / "auth.json"
    shutil.copyfile(src, home / "auth.json")
    tier = "" if service_tier in ("standard", "default") else f'service_tier = "{service_tier}"\n'
    (home / "config.toml").write_text(f'model = "{model}"\nmodel_reasoning_effort = "{effort}"\n'
                                      f'model_instructions_file = "{instructions}"\nproject_doc_max_bytes = 0\n{tier}')
    return home


def run_codex(prompt, model, effort, service_tier, events=None, timeout=3600, tries=2):
    dir_ = Path(tempfile.mkdtemp(prefix="translate-"))
    instructions = dir_ / "instructions.md"
    instructions.write_text(RULES)
    last = dir_ / "last.txt"
    home = codex_home(dir_, model, effort, instructions, service_tier)
    args = ["codex", "exec", "--ignore-rules", "--skip-git-repo-check", "--ephemeral", "-C", str(dir_), "-s", "read-only"]
    for f in OFF:
        args += ["--disable", f]
    args += ["--json", "-o", str(last), "-"]
    t0 = time.time()
    err = None
    for _ in range(tries):
        try:
            r = subprocess.run(args, input=prompt, capture_output=True, text=True, timeout=timeout,
                               env={**os.environ, "CODEX_HOME": str(home)})
        except subprocess.TimeoutExpired:
            err = f"codex exec timed out after {timeout}s"
            continue
        if events:
            Path(events).write_text(r.stdout)
        if r.returncode == 0 and last.exists():
            break
        err = f"codex exec exited {r.returncode}: {r.stderr[-2000:]}"
    else:
        raise RuntimeError(err)
    usage = None
    for line in r.stdout.splitlines():
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if ev.get("type") == "turn.completed":
            usage = ev.get("usage")
    return last.read_text(), usage, round(time.time() - t0)


def translate_one(work, meta, section, opt):
    text = (work / section["file"]).read_text()
    prompt = build_prompt(meta, section, text, opt.glossary_text)
    out = work / "md" / (Path(section["file"]).stem + ".md")
    events = work / "md" / (section["id"] + ".events.jsonl")
    problem = None
    for attempt in (1, 2):
        md, usage, seconds = run_codex(prompt, opt.model, opt.effort, opt.service_tier, events)
        problem = check_output(md, section.get("words"))
        if not problem:
            break
    if problem:
        (work / "md" / (section["id"] + ".rejected.md")).write_text(md)
        raise RuntimeError(f"{section['id']} {problem} (kept as {section['id']}.rejected.md)")
    out.write_text(md.strip() + "\n")
    return f"{section['id']} {section['title']}: {cjk_count(md)} chars, {seconds}s, usage {json.dumps(usage)}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("work")
    ap.add_argument("--only", help="comma-separated section ids")
    ap.add_argument("--jobs", type=int, default=20)
    ap.add_argument("--model", default="gpt-6-luna")
    ap.add_argument("--effort", default="low")
    ap.add_argument("--service-tier", default="priority")
    ap.add_argument("--glossary")
    ap.add_argument("--force", action="store_true", help="retranslate sections that already have an .md")
    ap.add_argument("--dry-run", action="store_true", help="print the first section's prompt and exit")
    opt = ap.parse_args()
    work = Path(opt.work).resolve()
    meta, sections = load_sections(work)
    (work / "md").mkdir(exist_ok=True)
    opt.glossary_text = Path(opt.glossary).read_text() if opt.glossary else None
    if opt.only:
        wanted = set(opt.only.split(","))
        sections = [s for s in sections if s["id"] in wanted]
    todo = [s for s in sections if opt.force or not (work / "md" / (Path(s["file"]).stem + ".md")).exists()]
    if opt.dry_run:
        s = todo[0] if todo else sections[0]
        print(RULES + "\n-----\n" + build_prompt(meta, s, (work / s["file"]).read_text(), opt.glossary_text))
        return
    print(f"{len(todo)} of {len(sections)} sections to translate with {opt.model}/{opt.effort}/{opt.service_tier}, "
          f"{opt.jobs} in flight", flush=True)
    failed = 0
    with ThreadPoolExecutor(max_workers=opt.jobs) as pool:
        futures = {pool.submit(translate_one, work, meta, s, opt): s for s in todo}
        for fut in as_completed(futures):
            try:
                print(fut.result(), flush=True)
            except Exception as e:
                failed += 1
                print(f"FAILED {futures[fut]['id']}: {e}", flush=True)
    done = sum(1 for s in sections if (work / "md" / (Path(s["file"]).stem + ".md")).exists())
    print(f"done: {done}/{len(sections)} sections translated, {failed} failed", flush=True)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
