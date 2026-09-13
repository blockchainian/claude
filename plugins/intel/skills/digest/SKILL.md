---
name: digest
description: >
  Turn a long-form source — an article, a podcast transcript, a YouTube video,
  a PDF (whitepaper, filing, deck), a page that only offers audio — into
  durable, searchable highlights. Use for "/digest <url-or-file>", "highlights
  of <url>", "what did <source> say about X", "summarize this
  article/episode/paper", "search my notes". Handles ordinary article and
  transcript pages, YouTube (via subtitles), PDFs (URL or local file), and
  audio pages (via the transcribe skill).

  NOT for: general web research across many pages (use agent-reach), or
  evaluating a tool or vendor (use evaluate).
---

# Digest — highlights from any source, stored and searchable

A source URL after `/digest` is fetched, read, and turned into a highlights
draft — that is the default. Two keywords instead select a store command.

| Argument | What it does |
|---|---|
| a source URL | Fetch the text, read it, write a highlights draft |
| `save` (none, or a draft path) | Store the item (no-op if already stored) |
| `save` take-aways (text) | Append them to the item's `## Take-aways` |
| `search` query (regex ok) | Search everything saved |

The store is `~/.claude/podcast-highlights/` (override with
`PODCAST_HIGHLIGHTS_DIR`). Items live in `episodes/<slug>.md`, listed in
`index.md`. Drafts stage in `.work/<slug>/` until saved. (The directory keeps
its original name; the store holds articles and episodes alike.)

`${CLAUDE_PLUGIN_ROOT}` below is this plugin's root; this skill lives at
`${CLAUDE_PLUGIN_ROOT}/skills/digest`.

## Setup (automatic, idempotent)

Run once at the start; it installs only what is missing and is a no-op when
everything is present, so it is safe to run every time.

```bash
bash "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/setup.sh"
```

It ensures `uv` (runs the trafilatura article extractor and the pdfminer PDF
extractor in an ephemeral env) and `yt-dlp` (YouTube subtitles).

## Digest a URL (the default)

1. **Fetch.**

   ```bash
   python3 "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/fetch_source.py" "<url>"
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

   A **PDF** (a `.pdf` URL, downloaded first, or a local file path passed
   straight in) is extracted with pdfminer. A PDF that comes back with almost
   no words is image-only (scanned) — there is no OCR here, so say it is scanned
   and stop rather than writing highlights from nothing.

1b. **Decide what the page gave you.**
   - If `audio_url` is **non-null** (the page links audio, or the URL itself was
     an audio file) and the text is thin, transcribe the audio with the
     `transcribe` skill. It is long-running (model download on first use,
     then faster than realtime), so run it in the **background** and wait:

     ```bash
     bash   "${CLAUDE_PLUGIN_ROOT}/skills/transcribe/scripts/setup.sh"
     python3 "${CLAUDE_PLUGIN_ROOT}/skills/transcribe/scripts/transcribe_audio.py" \
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

2. **Read all of it, in order.** Sequential chunks:
   `sed -n '1,90p' <transcript>`, then `91,200p`, and so on. Cap each call with
   `head -c 45000` so a chunk cannot blow up the context. Skimming the opening
   and the closing produces highlights that miss the middle, which is where the
   content usually is. If a chunk is sponsor reads, navigation cruft, or
   sign-off banter, note that and move on.

3. **Write the draft** to the `draft` path from step 1, using the template
   below. Then print the highlights in the conversation too — the user asked for
   highlights, not a file path.

4. Offer `/digest save` in one line. Do not save unprompted.

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
python3 "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/store.py" save "<draft path>"
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
   python3 "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/store.py" takeaway "<stored.md>" --list
   ```
3. **For each take-away in `<input>`** (strip any leading `1.`/`-`), decide:
   - **Overlaps an existing item** (same point, reworded or extended) → revise
     that item in place, merging the sharper wording:
     ```bash
     python3 "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/store.py" takeaway "<stored.md>" --revise <n> "<text>"
     ```
   - **New point** → append it:
     ```bash
     python3 "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/store.py" takeaway "<stored.md>" --add "<text>"
     ```

Judging overlap is yours — the script only edits the list. `--add`/`--revise`
print the resulting numbered take-aways; renumber against that before the next
call. Multiple `save <input>` calls accumulate into the same section.

## search

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/store.py" search "<query>"
python3 "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/store.py" list
```

The query is a case-insensitive regex over the whole file, frontmatter included,
so `search coinbase` finds it in `topics` as well as in the body. Report what
matched in your own words with the item and its URL; do not paste the raw match
block unless the user asks for it. Zero matches is an answer — say the store has
nothing on it, and offer to digest a source that would.
