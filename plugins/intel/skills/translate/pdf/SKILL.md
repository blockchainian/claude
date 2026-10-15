---
name: translate-pdf
description: >
  Translate an English document we print ourselves, a web page (course reading, lab handout, blog post), a Markdown
  note or a plain-text transcript, into a Chinese PDF in the translate-epub skill's book look: same page size, dark theme, Baskerville/Songti body,
  黑体 bold, boxed code, Mermaid diagrams, MathJax math, bookmarks. Code, identifiers, math and diagram labels stay
  English; prose, headings, captions and exercise text are translated through `codex exec`. Translating from the
  source keeps structure, code blocks, exercises and images exact. Use for "/translate-pdf <url or .md>", "把这个网页
  翻译成中文 PDF", "translate these course pages", or to re-render after editing a translation. NOT for a PDF that only
  exists as a PDF (papers, slides, scans: use translate-paper, translate-presentation or translate-scan), and NOT for an
  EPUB book (use translate-epub).
---

# Translate PDF

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The scripts import from the
sibling `translate-epub` and `fetch-x-mentions` skills, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_HOME` | Existing Codex login directory; default ~/.codex | No | Shell environment |
| `INTEL_STATE_DIR` | State root; work dirs go under `translate-pdf/`; default `~/.local/state/intel` | Optional | `~/.config/intel/.env` |
| `ISP_PROXY_URL` | Proxy for a page fetch that gets HTTP 429 | Optional | `~/.config/intel/.env` |

## Setup

```bash
"$SKILL_DIR/scripts/setup.sh"
```

It runs the translate-epub skill's setup (poppler, uv, codex login, Chrome) and installs Readability and jsdom for
`readable.mjs` into `scripts/node_modules`.

## Translate one document

```bash
# A Markdown note (Mermaid fences become diagrams)
"$SKILL_DIR/scripts/translate_doc.sh" --md note.md --out ~/Documents/zh/note.pdf

# A plain-text lecture transcript, one sentence per line, regrouped into paragraphs of about 150 words
"$SKILL_DIR/scripts/translate_doc.sh" --txt lecture.txt --out ~/Documents/zh/lecture.pdf

# A course page, printed as it is
"$SKILL_DIR/scripts/translate_doc.sh" --url https://example.edu/classes/02-testing/ --out ~/Documents/zh/L02.pdf

# A blog post or vendor doc: keep only the article
"$SKILL_DIR/scripts/translate_doc.sh" --url https://blog.example.com/post --reader --out ~/Documents/zh/post.pdf
```

A 10k-word page takes about a minute: eight codex calls of about 1,500 words run at once.

- `--title` names the document in the prompt (default: the output's file name); `--context` says what it is, such
  as "MIT 6.1020 course reading". Both set the translator's register.
- `--glossary FILE` pins terms, one `english: 中文` line each. Keep one glossary per course or subject and pass it to
  every document of it, so terms agree across files; pin any heading that repeats on every page too. Course
  glossaries shared with the other translate-epub skills live in
  `~/.local/state/intel/translate-glossaries/<course number>.md`; translate-paper appends to them.
- `--reader` keeps only the article on a page whose site wraps it in chrome: tags, dates, avatars, reading time,
  share bars, subscription boxes, site footers. A course site whose page is the document, exercise widgets
  included, goes without it, since Readability would drop the exercises.
- `--html FILE` uses a saved page instead of fetching; `--url` still resolves its links and images.
- `--drop-heading TEXT` removes each section whose heading contains TEXT, such as a login-only tutor section.
- `--skip-bookmark TEXT` drops outline entries titled TEXT, such as the "阅读练习" heading every exercise block
  repeats.
- `--work DIR` sets the work dir (default `$INTEL_STATE_DIR/translate-pdf/<name>`).

## How it works

1. Split. `md_segments.py` cuts Markdown into runs of non-blank lines; fenced code and Mermaid are never sent, and a
   heading's `#` is restored afterwards. `html_segments.py` strips navigation, page chrome, footers, licence badges
   and exercise answers, then takes each block that holds text but no nested block, or a run of loose text beside
   blocks; `pre`, code-only text, scripts and SVG are never sent. Each tag's attributes become `k="n"`, so the model
   sees short tags, and join puts them back. Segment ids are hashes of their text, so the cache survives a changed
   page.
2. Translate. `translate_segments.mjs` sends consecutive segments as one JSON object per `codex exec` call through
   the translate-epub skill's `runCodex` and checks each answer: every link target, `k="n"` and whole code element must
   come back; on a page that loads MathJax or KaTeX no formula may run into Chinese text (a lost delimiter); where
   the source has no TeX, none may be added. A failing segment is retried in a smaller batch, then alone at medium
   effort; one that fails all three stays English and is reported `FAILED`.
3. Print. `html2pdf.py`, or `md2pdf.py` for Markdown, prints with headless Chrome at 427.92 × 660 pt: `lang=zh-CN`,
   9.25pt body at line-height 1.8, paragraphs indented 2em outside lists, tables and exercises, 黑体 for bold and
   headings, emphasis as 黑体 for Chinese and Baskerville italic for Latin (so `<em>a</em>` still reads as a math
   variable), Latin words hyphenated as English, wide code and images fitted to the column. It waits for MathJax to
   finish, and a print that never settles (a slow server holding the load event) is retried once.

## Check the result

- A `FAILED` line in the run's output: read the segment in `<work>/zh.rejected.json`. A sound translation the checks
  reject (a model that writes `delete()` for the source's `delete` every time) is accepted by copying it into
  `zh.json` and adding its id to `<work>/zh.accepted.json`, a JSON list the recheck skips.
- A segment still in English: its source has prose outside code, but its `zh.json` entry has no Chinese. Function
  signatures, names and code drawn as text legitimately stay English.
- Raw TeX in `pdftotext` output (`$x$`, `\(`, `\begin{`) on a math page means MathJax did not typeset; rerun.
- Look at the first and last pages: a page's last screen is where site chrome hides.

## Batches

Write a small driver that maps each document to its source and options (glossary, `--reader` by host, the output
path) and runs `translate_doc.sh` a few at a time, each run logging to its own file. Each run holds a headless Chrome
and a pymupdf check, so run at most 3 at once, and never alongside another translate-epub skill's batch: concurrent
batches exhausted memory and rebooted the machine. Never edit the driver while it runs: bash reads a script as it executes, so running instances pick up
shifted bytes. A second full pass is cheap, since every translated segment is cached: it rechecks everything, redoes
only what fails, and re-renders.

## Edit a finished translation

Fix the text in `<work>/zh.json`, or in the joined `<work>/<name>.md` for Markdown, and rerun the same
`translate_doc.sh` command; it keeps every segment that passes the checks.
