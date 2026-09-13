---
name: podcast
description: >
  Turn a podcast transcript URL into durable highlights, and search everything
  saved so far. Use for "/podcast", "/podcast read <url>", "highlights of
  <podcast url>", "what did <show> say about X", "summarize this episode",
  "search my podcast notes".
  Handles ordinary transcript pages and YouTube (via subtitles).

  NOT for: general web research (use agent-reach), evaluating a tool or
  vendor (use evaluate), or reading an article that is not an episode.
---

# Podcast — highlights from transcripts, stored and searchable

Three commands. The argument after `/podcast` selects one; a bare URL means
`read`.

| Command | Argument | What it does |
|---|---|---|
| `read` | transcript URL | Fetch the transcript, read it, write a draft |
| `save` | none (or a draft path) | Store the episode (no-op if already stored) |
| `save` | take-aways (text) | Append them to the episode's `## Take-aways` |
| `search` | query (regex ok) | Search every saved episode |

Store location: `~/.claude/podcast-highlights/` (override with
`PODCAST_HIGHLIGHTS_DIR`). Episodes live in `episodes/<slug>.md`, listed in
`index.md`. Drafts stage in `.work/<slug>/` until saved.

`${CLAUDE_PLUGIN_ROOT}` below is this plugin's root; this skill lives at
`${CLAUDE_PLUGIN_ROOT}/skills/podcast`.

## read

1. **Fetch.**

   ```bash
   python3 "${CLAUDE_PLUGIN_ROOT}/skills/podcast/scripts/fetch_transcript.py" "<url>"
   ```

   It prints JSON with `slug`, `transcript`, `draft`, `words`, `thin` and
   `audio_url`. A dead link or a blocked request exits with the HTTP status —
   report that status, do not retry the same URL.

   For a YouTube URL it pulls subtitles with `yt-dlp` and reports which kind
   in `subtitles`. With `auto` there are no speaker labels, so attribute
   quotes to the show, not to a named host; with `manual` the labels may be
   present, so use them only where the transcript actually carries them.

1b. **Thin page → transcribe the audio if there is any.** A `thin: true`
   result (under 1500 words) means the page had no real transcript — a player,
   a paywall or a show-notes blurb.
   - If `audio_url` is **non-null** (the page links episode audio, or the URL
     itself was an audio file), transcribe it with the `transcribe-audio`
     skill. It is long-running (model download on first use, then faster than
     realtime), so run it in the **background** and wait for it:

     ```bash
     python3 "${CLAUDE_PLUGIN_ROOT}/skills/transcribe-audio/scripts/transcribe_audio.py" \
       "<audio_url>" "<transcript path>"
     ```

     It writes plain text to the same `transcript` path and prints JSON with
     the new `words`/`thin`. See the `transcribe-audio` skill for its
     requirements and caveats — in short: needs `uv` or `mlx_whisper` (Apple
     Silicon); the transcript has **no speaker labels**, so attribute quotes to
     the show, not a named host; and the audio often carries
     **dynamically-inserted modern ads** with whisper occasionally looping on a
     garbled stretch — note both and exclude/repair them when reading.
   - If `audio_url` is **null**, there is nothing to transcribe — say the page
     was a player/paywall and stop.

2. **Read all of it, in order.** Sequential chunks:
   `sed -n '1,90p' <transcript>`, then `91,200p`, and so on. Cap each call
   with `head -c 45000` so a chunk cannot blow up the context.
   Skimming the opening and the closing produces highlights that miss the
   middle, which is where the episode's actual content is. If a chunk is
   sponsor reads, listener mail, or sign-off banter, note that and move on.

3. **Write the draft** to the `draft` path from step 1, using the template
   below. Then print the highlights in the conversation too — the user asked
   for highlights, not for a file path.

4. Offer `/podcast save` in one line. Do not save unprompted.

### What makes a highlight

- **Organise by theme, not by timestamp.** The transcript is already
  chronological; that ordering is not a finding.
- **Keep the specifics.** Numbers, dates, company and product names, dollar
  amounts, who was wrong about what. A highlight that survives paraphrase
  into "they discussed strategy" was not a highlight.
- **Record the disagreements and the misses**, not only the thesis. Hosts
  hedging against each other is signal.
- **Quote sparingly** — a handful of short lines, each attributed to the
  speaker, only where the wording itself is the point. Everything else is
  your own compression. Never reproduce long stretches of the transcript.
- **Claim nothing the transcript does not say.** No filling gaps from
  background knowledge; if the episode leaves something open, say it is open.
- Length scales with the episode: a 3-hour episode earns more than a 30-minute
  interview, but padding is worse than brevity in both.

### Draft template

```markdown
---
title: <episode title>
show: <podcast name>
url: <source url>
slug: <the slug from step 1>
episode: <season/number, or omit>
aired: <YYYY-MM or YYYY-MM-DD if stated, or omit>
topics: [<3-8 lowercase search keys: companies, people, concepts>]
---

# <Show> — <Episode title>

<One line: what this episode is and what period or subject it covers.>

## <Theme>
- <point>

## <Theme>
- <point>

## Quotes
> "<short line>" — <Speaker>

## TL;DR
- <at most 5 bullets, 10 words each, plain words>
```

`topics` is what makes `search` useful later — put the names someone would
search for in six months, not generic category words.

## save

`save` has two forms. Bare `save` stores the episode; `save <input>` attaches
take-aways to it.

### save (no input) — store the highlights

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/podcast/scripts/store.py" save "<draft path>"
```

Save the draft from the `read` run in this session (no draft in the session →
ask which one rather than guessing). The script refuses a draft missing
`title`, `show`, or `url`, stamps `saved:` with today's date, and rebuilds
`index.md`. A different episode landing on the same slug is filed alongside it
as `<slug>-2.md`. **If this episode (same `url`) is already stored, save is a
no-op** — it prints the stored path and changes nothing, so it never clobbers
take-aways added later. Read the path the script prints — a `-2` means two
episodes share a slug.

### save \<input\> — append take-aways

`<input>` is one or more of the user's own take-aways (often a numbered list).
Each becomes a bullet in a `## Take-aways` section at the top of the stored
episode file. Steps:

1. **Resolve the stored file.** Run bare `save` first (it stores the episode,
   or no-ops and prints the path if already stored). Use that printed path as
   `<stored.md>`. No draft this session → find the episode with `search`/`list`.
2. **See what's already there:**
   ```bash
   python3 "${CLAUDE_PLUGIN_ROOT}/skills/podcast/scripts/store.py" takeaway "<stored.md>" --list
   ```
3. **For each take-away in `<input>`** (strip any leading `1.`/`-`), decide:
   - **Overlaps an existing item** (same point, reworded or extended) → revise
     that item in place, merging the sharper wording:
     ```bash
     python3 "${CLAUDE_PLUGIN_ROOT}/skills/podcast/scripts/store.py" takeaway "<stored.md>" --revise <n> "<text>"
     ```
   - **New point** → append it:
     ```bash
     python3 "${CLAUDE_PLUGIN_ROOT}/skills/podcast/scripts/store.py" takeaway "<stored.md>" --add "<text>"
     ```

Judging overlap is yours — the script only edits the list. `--add`/`--revise`
print the resulting numbered take-aways; renumber against that before the next
call. Multiple `save <input>` calls accumulate into the same section.

## search

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/podcast/scripts/store.py" search "<query>"
python3 "${CLAUDE_PLUGIN_ROOT}/skills/podcast/scripts/store.py" list
```

The query is a case-insensitive regex over the whole file, frontmatter
included, so `search coinbase` finds it in `topics` as well as in the body.
Report what matched in your own words with the episode and its URL; do not
paste the raw match block unless the user asks for it. Zero matches is an
answer — say the store has nothing on it, and offer to run `read` on
an episode that would.
