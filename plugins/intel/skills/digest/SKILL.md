---
name: digest
description: >
  Turn a long-form source — an article, a podcast transcript, a YouTube video,
  a PDF (whitepaper, filing, deck), a page that only offers audio — into
  durable, searchable highlights. Use for "/digest <url-or-file>", "highlights
  of <url>", "what did <source> say about X", "summarize this
  article/episode/paper", "search my notes". Handles ordinary article and
  transcript pages, YouTube (via subtitles), PDFs (URL or local file), and
  audio pages (via the transcribe skill). A PDF is highlighted chapter by
  chapter and the highlights come back as a PDF in the translate skill's book
  format, at the source's page size.

  NOT for: general web research across many pages (use agent-reach), or
  evaluating a tool or vendor (use evaluate).
---

# Digest

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts use the sibling `fetch-x-mentions`, `transcribe` and `translate` skills, so keep the whole
intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_OUTPUT_DIR` | Output root; the store is `digests/` under it; default `~/Documents` | No | ~/.config/intel/.env |
| `INTEL_STATE_DIR` | State root; drafts stage in `digest/<slug>/` under it until saved; default `~/.local/state/intel` | No | ~/.config/intel/.env |

## Setup

Run the setup script at the start of every run. It installs only what is missing: `uv` (which runs
the trafilatura article extractor and the PDF scripts), `yt-dlp` for YouTube subtitles and
`poppler` for `pdftotext`. Typesetting a highlights PDF also needs Google Chrome.

```bash
"$SKILL_DIR/scripts/setup.sh"
```

## Modes

A source URL after `/digest` is the default: it is fetched, read and turned into a highlights
draft. Two keywords select a store command instead.

| Argument | What it does |
|---|---|
| a source URL | Fetch the text, read it, write a highlights draft |
| a PDF (file or `.pdf` URL) | Highlights per chapter, typeset as `<name>-highlights.pdf` |
| `save` (none, or a draft path) | Store the item (no-op if already stored) |
| `save` take-aways (text) | Append them to the item's `## Take-aways` |
| `search` query (regex ok) | Search everything saved |

The store holds articles, episodes, videos and papers alike. Each item lives in `items/<slug>.md`
and is listed in `index.md`.

## Digest a URL

A PDF, whether a `.pdf` URL or a local file, skips this flow; follow "Digest a PDF" instead. For
any other URL, fetch it first:

```bash
"$SKILL_DIR/scripts/fetch_source.py" "<url>"
```

The script prints JSON with `slug`, `transcript` (the text file path), `draft`, `words`, `thin` and
`audio_url`. An ordinary page yields only its main article body, without nav, sidebars or footers.
A YouTube URL yields its subtitles, and `subtitles` says which kind: with `auto` there are no
speaker labels, so attribute quotes to the source rather than a named speaker; with `manual`,
use speaker labels only where the text actually carries them.

Next, decide what the page gave you. If `audio_url` is non-null (the page links audio, or the URL
was itself an audio file) and the text is thin, transcribe the audio with the `transcribe` skill.
It is long-running, with a model download on first use and faster than realtime after that, so
run it in the background and wait for it to finish before reading the transcript; a subagent
waits on its own command before returning.

```bash
"$SKILL_DIR/../transcribe/scripts/setup.sh"
"$SKILL_DIR/../transcribe/scripts/transcribe-audio.mjs" \
  "<audio_url>" "<transcript path>"
```

It writes plain text to the same `transcript` path and prints JSON with the new `words` and
`thin`. A transcribed transcript has no speaker labels. Downloaded audio often carries
dynamically inserted modern ads, and whisper occasionally loops on a garbled stretch; note both
and exclude or repair them as you read. The `transcribe` skill has more.

Otherwise, read the text you have. `thin` (under 1500 words) is only a hint: a short article is
still a real article, so highlight it. But if the page is a bare player or paywall shell with no
real prose and no `audio_url`, say so and stop rather than inventing highlights.

Read all of the transcript, in order, in sequential chunks with the Read tool (`offset: 1, limit:
90`, then `offset: 91, limit: 110`, and so on), keeping each chunk under roughly 45000 characters
so it cannot blow up the context. Skimming the opening and the closing misses the middle, which
is usually where the content is. When a chunk is sponsor reads, navigation cruft or sign-off
banter, note that and move on.

Write the draft to the `draft` path from the fetch output, using the draft template below, then
print the highlights in the conversation too: the user asked for highlights, not a file path.
Offer `/digest save` in one line, and do not save unprompted.

## Digest a PDF

A PDF gets one set of highlights per chapter, and the result is itself a PDF in the translate
skill's book format (dark page, Baskerville + Songti SC, chapter openers, running heads, folios,
one bookmark per chapter) at the source PDF's own page size. Split it first:

```bash
"$SKILL_DIR/scripts/pdf_highlights.py" split "<file.pdf or url>"
```

The script prints JSON with `work` (the work dir), `unit`, `page_size` and `chapters`, each with
`id`, `title`, `pages`, `chars`, `text` and `highlights` (paths relative to `work`). The chapters
are the PDF's bookmarks. A PDF with no bookmarks is split one section per page (`unit: "page"`),
and `--by page` forces that.

From the split JSON, pick the chapters that carry real content. Skip, and do not dispatch, any
that is front or back matter (cover, contents page, index, copyright page) or has a near-zero
`chars`; a skipped chapter gets no highlights file, and `render` ignores it.

A book (`unit: "chapter"`) is written in parallel, one fresh subagent per content chapter, which
is faster and keeps each chapter's text out of your own context. Use Claude Code's Agent tool or
Codex's `spawn_agent` with `fork_turns: "none"`, since each subagent needs only its own chapter,
never this conversation. Launch several in one message so they run at once; for a long book keep
each batch to about 6–8 subagents and launch the next batch when the first returns. In Codex,
collect each child's final result with `wait_agent`. Give every subagent a prompt containing,
filled in for its chapter:

- the absolute paths to read (`<work>/<text>`) and to write (`<work>/<highlights>`), the chapter
  `title` and the `unit`;
- this instruction: *Read all of the text file in chunks (the host's file-reading tool, bounded
  chunks of ~45000 chars — do not skim the middle), then write the highlights file: a `# <title>`
  line followed by themed `## ` sections. No frontmatter, no TL;DR. Write in the language of the
  PDF. Reply only `done`, or the error if you could not write the file.*
- the "Writing a chapter" and "What makes a highlight" sections below, both copied verbatim:
  together they are the whole brief, since the subagent cannot see this skill.

When the subagents return, check with `ls -l` that every expected `<work>/<highlights>` exists and
is non-empty, and re-dispatch any that are missing or empty before rendering.

A per-page PDF (`unit: "page"`: a deck, a filing, a form) is not fanned out, because its sections
are single pages and there can be a great many. Write those yourself, in order, as you read them.

Then render:

```bash
"$SKILL_DIR/scripts/pdf_highlights.py" render "<work>"
```

It typesets every chapter that has a highlights file into `<source>-highlights.pdf` in the store,
prints that path and writes the combined `<work>/draft.md`. `--out`, `--bg`, `--fg` and
`--font-size` override the defaults, which are the translate skill's. Re-run it after editing any
chapter's file.

Report the PDF path and a short per-chapter summary in the conversation. Fill `source`, `author`
and `topics` into the frontmatter of `draft.md`, then offer `/digest save` in one line. Do not
save unprompted.

### Writing a chapter

A book (`unit: "chapter"`) is written as prose, not bullets: a book is long, and a chapter of
bullet points reads as disconnected notes. Open the chapter with one paragraph stating what it
argues, then make each `## ` section one to a few complete paragraphs that read straight through.
Each paragraph carries one line of the argument, its sentences are connected (because, so, but,
as a result), and the numbers, names and dates sit inside the sentences. Use no bullet lists and
no `## Quotes` section; a short quote goes inside the paragraph it belongs to, attributed there.
The rules in "What makes a highlight" still hold: theme over order, specifics kept, disagreements
recorded, nothing the source does not say. Only a PDF split per page (`unit: "page"`: a deck, a
filing, a form) keeps themed bullets and an optional `## Quotes`, as do articles, podcasts and
videos in the URL flow.

## What makes a highlight

- **Organise by theme, not by order.** The source is already sequential; that
  ordering is not a finding.
- **Keep the specifics.** Numbers, dates, company and product names, dollar
  amounts, who was wrong about what. A highlight that survives paraphrase into
  "they discussed strategy" was not a highlight.
- **Record the disagreements and the misses**, not only the thesis. A writer
  hedging, or speakers pushing back on each other, is signal.
- **Quote sparingly** — a handful of short lines, attributed where a speaker or
  author is identifiable, only where the wording itself is the point. Everything
  else is your own compression. Never reproduce long stretches of the source.
- **Claim nothing the source does not say.** No filling gaps from background
  knowledge; if it leaves something open, say it is open.
- Length scales with the source: a 3-hour episode or a 5000-word essay earns
  more than a short post, but padding is worse than brevity in both.

<example>
Excerpt: "We shipped the rewrite in March. Cold start went from 2.1 seconds
to 400 milliseconds. Honestly, the migration was a disaster for two months —
we lost a third of the team's time to flaky tests."

Highlights written from it:

## The rewrite
- Shipped in March; cold start fell from 2.1s to 400ms.
- The migration cost roughly two months, with a third of the team's time
  going to flaky tests.

## Quotes
> "the migration was a disaster for two months" — the host

Why: the one passage kept in the source's own wording is marked as a quote;
everything not inside quote marks is paraphrase.
</example>

## Draft template

```markdown
---
title: <title>
source: <site, publication, or podcast/show name>
url: <source url>
slug: <the slug from step 1>
author: <author or host, if known — else omit>
published: <YYYY-MM or YYYY-MM-DD if stated, or omit>
topics: [<3-8 lowercase search keys: companies, people, concepts>]
---

# <Source> — <Title>

<One line: what this is and what period or subject it covers.>

## <Theme>
- <point>

## <Theme>
- <point>

## Quotes
> "<short line>" — <Speaker or author, if identifiable>

## TL;DR
- <at most 5 bullets, 10 words each, plain words>
```

`title` and `url` are required to save; the rest are optional. `slug` is the one the fetch printed.
`topics` is what makes `search` useful later, so put in the names someone would search for in six
months, not generic category words.

## Save

Bare `save` stores the draft from this session's digest run; if there is no draft in the session,
ask which one rather than guessing.

```bash
"$SKILL_DIR/scripts/store.mjs" save "<draft path>"
```

The script refuses a draft missing `title` or `url`, and stamps `saved:` with today's date. If
the same `url` is already stored, save is a no-op: it prints the stored path and changes nothing,
so it never clobbers take-aways added later. A different item landing on the same slug is filed
beside it as `<slug>-2.md`, so read the path the script prints: a `-2` means two items share a
slug.

`save <input>` attaches the user's own take-aways, often a numbered list, to the stored item, each
as a bullet in a `## Take-aways` section at the top of the file. Repeated calls accumulate into
the same section.

1. Resolve the stored file by running bare `save` first, which stores the item or prints its path
   if already stored, and use that path as `<stored.md>`. With no draft this session, find the
   item with `search` or `list`.
2. See what is already there:
   ```bash
   "$SKILL_DIR/scripts/store.mjs" takeaway "<stored.md>" --list
   ```
3. For each take-away in `<input>`, with any leading `1.` or `-` stripped, decide yourself whether
   it overlaps an existing item; the script only edits the list. If it makes the same point,
   reworded or extended, revise that item in place with the sharper wording merged in; if it is
   a new point, append it:
   ```bash
   "$SKILL_DIR/scripts/store.mjs" takeaway "<stored.md>" --revise <n> "<text>"
   "$SKILL_DIR/scripts/store.mjs" takeaway "<stored.md>" --add "<text>"
   ```
   Both print the resulting numbered take-aways; renumber against that before the next call.

## Search

```bash
"$SKILL_DIR/scripts/store.mjs" search "<query>"
"$SKILL_DIR/scripts/store.mjs" list
```

The query is a case-insensitive regex over the whole file, frontmatter included, so `search
coinbase` finds it in `topics` as well as in the body. Report what matched in your own words, with
the item and its URL, and paste the raw match block only if the user asks for it. Zero matches is
an answer: say the store has nothing on it, and offer to digest a source that would.

## Failures and limits

- A dead link or a blocked request makes `fetch_source.py` exit with the HTTP status. Report that
  status, and do not retry the same URL.
- An image-only (scanned) PDF makes `split` exit with an error. There is no OCR here, so say it is
  scanned and stop.

## Tests

```bash
uv run "$SKILL_DIR/tests/test_fetch_source.py"
uv run "$SKILL_DIR/tests/test_pdf_highlights.py"
node --test "$SKILL_DIR"/tests/*.mjs
```
