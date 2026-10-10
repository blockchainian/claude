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

# Digest — highlights from any source, stored and searchable

## Runtime and paths

These skills run in Claude Code and Codex. Set `SKILL_DIR` to the absolute directory of this
loaded `SKILL.md`, not the working directory or a host environment variable, and repeat it, with
any other variable a command below uses, in every shell call:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

If the loaded path is unavailable, stop and report it. Keep the whole intel plugin installed,
because sibling skills share scripts.

For a long-running command, choose a deadline before launch and keep the process handle and its
output. In Claude Code run it with `run_in_background` and wait for the completion notification;
in Codex keep the shell tool's session handle and wait on it. A subagent waits for its own commands
before returning. Do not poll logs in a loop. On a timeout, keep the diagnostics and report the
process state before retrying.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_OUTPUT_DIR` | Output root; digests go under `digests/`; default `~/Documents` | No | ~/.config/intel/.env |

A source URL after `/digest` is fetched, read, and turned into a highlights
draft — that is the default. Two keywords instead select a store command.

| Argument | What it does |
|---|---|
| a source URL | Fetch the text, read it, write a highlights draft |
| a PDF (file or `.pdf` URL) | Highlights per chapter, typeset as `<name>-highlights.pdf` |
| `save` (none, or a draft path) | Store the item (no-op if already stored) |
| `save` take-aways (text) | Append them to the item's `## Take-aways` |
| `search` query (regex ok) | Search everything saved |

The store is `digests/` under the output root. Items live in `items/<slug>.md`, listed in
`index.md`. Drafts stage in `~/.local/state/intel/digest/<slug>/` (`INTEL_STATE_DIR`) until saved. (The store holds
articles, episodes, videos and papers alike.)

## Setup (automatic, idempotent)

Run once at the start; it installs only what is missing and is a no-op when
everything is present, so it is safe to run every time.

```bash
"$SKILL_DIR/scripts/setup.sh"
```

It ensures `uv` (runs the trafilatura article extractor and the PDF scripts
in an ephemeral env), `yt-dlp` (YouTube subtitles) and `poppler` (`pdftotext`,
for PDFs). Typesetting a highlights PDF also needs Google Chrome.

## Digest a URL (the default)

1. **Fetch.**

   ```bash
   "$SKILL_DIR/scripts/fetch_source.py" "<url>"
   ```

   It prints JSON with `slug`, `transcript` (the text file path), `draft`,
   `words`, `thin` and `audio_url`. For an ordinary page it extracts the main
   article body with trafilatura (falling back to a plain tag-strip), so nav,
   sidebars and footers are dropped. A dead link or a blocked request exits with
   the HTTP status — report that status, do not retry the same URL.

   For a YouTube URL it pulls subtitles with `yt-dlp` and reports which kind in
   `subtitles`. With `auto` there are no speaker labels, so attribute quotes to
   the source, not a named speaker; with `manual` the labels may be present, so
   use them only where the text actually carries them.

   A **PDF** (a `.pdf` URL or a local file path) does not go through this
   step — follow "Digest a PDF" below instead.

1b. **Decide what the page gave you.**
   - If `audio_url` is **non-null** (the page links audio, or the URL itself was
     an audio file) and the text is thin, transcribe the audio with the
     `transcribe` skill. It is long-running (model download on first use,
     then faster than realtime), so follow the host-specific long-command
     instructions in Runtime and paths. Wait for completion before reading
     the transcript; a subagent awaits its own command before returning:

     ```bash
     "$SKILL_DIR/../transcribe/scripts/setup.sh"
     "$SKILL_DIR/../transcribe/scripts/transcribe-audio.mjs" \
       "<audio_url>" "<transcript path>"
     ```

     It writes plain text to the same `transcript` path and prints JSON with the
     new `words`/`thin`. A transcribed transcript has **no speaker labels**, and
     downloaded audio often carries **dynamically-inserted modern ads** with
     whisper occasionally looping on a garbled stretch — note both and
     exclude/repair them when reading. See the `transcribe` skill for more.
   - Otherwise, **read the text you have.** `thin` (under 1500 words) is only a
     hint: a short *article* is still a real article — highlight it. But if the
     page is a bare player or paywall shell with no real prose and no
     `audio_url`, say so and stop rather than inventing highlights.

2. **Read all of it, in order.** Sequential chunks with the Read tool:
   `offset: 1, limit: 90` on the transcript, then `offset: 91, limit: 110`, and
   so on. Keep each chunk's `limit` small enough that it cannot blow up the
   context — roughly 45000 characters. Skimming the opening
   and the closing produces highlights that miss the middle, which is where the
   content usually is. If a chunk is sponsor reads, navigation cruft, or
   sign-off banter, note that and move on.

3. **Write the draft** to the `draft` path from step 1, using the template
   below. Then print the highlights in the conversation too — the user asked for
   highlights, not a file path.

4. Offer `/digest save` in one line. Do not save unprompted.

## Digest a PDF (per chapter, PDF out)

A PDF gets one set of highlights per chapter — connected paragraphs for a
book, bullets for a per-page PDF — and the result is itself a PDF:
the translate skill's book format (dark page, Baskerville + Songti SC, chapter
openers, running heads, folios, one bookmark per chapter) at the source PDF's
own page size.

1. **Split.**

   ```bash
   "$SKILL_DIR/scripts/pdf_highlights.py" split "<file.pdf or url>"
   ```

   It prints JSON: `work` (the work dir), `unit`, `page_size`, and `chapters`,
   each with `id`, `title`, `pages`, `chars`, `text` and `highlights` (paths
   relative to `work`). The chapters are the PDF's bookmarks; a PDF with no
   bookmarks is split one section per page (`unit: "page"`), and `--by page`
   forces that. An image-only (scanned) PDF exits with an error — there is no
   OCR here, so say it is scanned and stop.

2. **Write one highlights file per chapter — in parallel, one subagent per
   chapter.** Each chapter is independent: its own text file in, its own
   highlights file out. Fanning them out is faster than writing them one by one,
   and it keeps each chapter's text out of your own context.

   First, from the split JSON, pick the chapters that carry real content. **Skip
   — do not dispatch —** any that is front or back matter (cover, contents page,
   index, copyright page) or has a near-zero `chars`; a skipped chapter simply
   gets no highlights file, and `render` ignores it.

   Then dispatch the content chapters with Claude Code's Agent tool or Codex's
   `spawn_agent` (`fork_turns: "none"`): **one fresh subagent per chapter** (each subagent needs only
   its own chapter, never this conversation). Launch several in one message so
   they run at once; for a long book keep each batch to about 6–8 subagents and
   launch the next batch when the first returns. Give every subagent a prompt
   that contains, filled in for its chapter:
   - the absolute paths to read (`<work>/<text>`) and to write
     (`<work>/<highlights>`), the chapter `title`, and the `unit`;
   - this instruction: *Read all of the text file in chunks (the host's file-reading tool,
     bounded chunks of ~45000 chars — do not skim the middle), then write the
     highlights file: a `# <title>` line followed by themed `## ` sections. No
     frontmatter, no TL;DR. Write in the language of the PDF. Reply only `done`,
     or the error if you could not write the file.*
   - the **"Writing a chapter"** block and the **"What makes a highlight"**
     section below, both copied verbatim — together they are the whole brief the
     subagent writes to, since it cannot see this skill.

   In Claude Code collect Agent results; in Codex use `wait_agent` and collect
   each child's final result. When the subagents return, verify every expected `<work>/<highlights>` exists
   and is non-empty (`ls -l`); re-dispatch any that are missing or empty before
   rendering.

   A **per-page PDF** (`unit: "page"`: a deck, a filing, a form) is **not** fanned
   out — its sections are single pages and there can be a great many, so write
   those yourself, in order, as you read them.

### Writing a chapter

   **A book (`unit: "chapter"`) is written as prose, not bullets.** A book is
   long, and a chapter of bullet points reads as disconnected notes. Open the
   chapter with one paragraph stating what it argues, then make each `## `
   section one to a few complete paragraphs that read straight through: each
   paragraph carries one line of the argument, its sentences are connected
   (because, so, but, as a result), and the numbers, names and dates sit
   inside the sentences. No bullet lists and no `## Quotes` section — a short
   quote goes inside the paragraph it belongs to, attributed there. The rules
   in "What makes a highlight" still hold: theme over order, specifics kept,
   disagreements recorded, nothing the source does not say. Only a PDF split
   per page (`unit: "page"`: a deck, a filing, a form) keeps themed bullets and
   an optional `## Quotes`; so do articles, podcasts and videos in the URL
   flow above.

3. **Render.**

   ```bash
   "$SKILL_DIR/scripts/pdf_highlights.py" render "<work>"
   ```

   It typesets every chapter that has a highlights file into
   `<source>-highlights.pdf` in the store, prints that path, and
   writes the combined `<work>/draft.md`. `--out`, `--bg`, `--fg` and
   `--font-size` override the defaults, which are the translate skill's.
   Re-run it after editing any chapter's file.

4. Report the PDF path and a short per-chapter summary in the conversation.
   Fill `source`, `author` and `topics` into the frontmatter of `draft.md`,
   then offer `/digest save` in one line. Do not save unprompted.

### What makes a highlight

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

### Draft template

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

`title` and `url` are required to save; the rest are optional. `topics` is what
makes `search` useful later — put the names someone would search for in six
months, not generic category words.

## save

`save` has two forms. Bare `save` stores the item; `save <input>` attaches
take-aways to it.

### save (no input) — store the highlights

```bash
"$SKILL_DIR/scripts/store.mjs" save "<draft path>"
```

Save the draft from the digest run in this session (no draft in the session →
ask which one rather than guessing). The script refuses a draft missing `title`
or `url`, stamps `saved:` with today's date, and rebuilds `index.md`. A
different item landing on the same slug is filed alongside it as `<slug>-2.md`.
**If this item (same `url`) is already stored, save is a no-op** — it prints the
stored path and changes nothing, so it never clobbers take-aways added later.
Read the path the script prints — a `-2` means two items share a slug.

### save \<input\> — append take-aways

`<input>` is one or more of the user's own take-aways (often a numbered list).
Each becomes a bullet in a `## Take-aways` section at the top of the stored
file. Steps:

1. **Resolve the stored file.** Run bare `save` first (it stores the item, or
   no-ops and prints the path if already stored). Use that printed path as
   `<stored.md>`. No draft this session → find the item with `search`/`list`.
2. **See what's already there:**
   ```bash
   "$SKILL_DIR/scripts/store.mjs" takeaway "<stored.md>" --list
   ```
3. **For each take-away in `<input>`** (strip any leading `1.`/`-`), decide:
   - **Overlaps an existing item** (same point, reworded or extended) → revise
     that item in place, merging the sharper wording:
     ```bash
     "$SKILL_DIR/scripts/store.mjs" takeaway "<stored.md>" --revise <n> "<text>"
     ```
   - **New point** → append it:
     ```bash
     "$SKILL_DIR/scripts/store.mjs" takeaway "<stored.md>" --add "<text>"
     ```

Judging overlap is yours — the script only edits the list. `--add`/`--revise`
print the resulting numbered take-aways; renumber against that before the next
call. Multiple `save <input>` calls accumulate into the same section.

## search

```bash
"$SKILL_DIR/scripts/store.mjs" search "<query>"
"$SKILL_DIR/scripts/store.mjs" list
```

The query is a case-insensitive regex over the whole file, frontmatter included,
so `search coinbase` finds it in `topics` as well as in the body. Report what
matched in your own words with the item and its URL; do not paste the raw match
block unless the user asks for it. Zero matches is an answer — say the store has
nothing on it, and offer to digest a source that would.
