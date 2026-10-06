# intel

Gather and distill knowledge from **long-form sources** — articles, podcasts,
talks, videos, live streams — and from **what people say** about an app — App
Store reviews — into text you can read and search. The plugin also finds book
download links and checks domain names.

## Hosts and install

Claude Code loads all fifteen skills. Codex loads fourteen; `case-study` keeps its
Claude Workflow implementation and is excluded by Intel's Codex manifest.

```sh
# Claude Code
/plugin marketplace add blockchainian/claude
/plugin install intel@blockchainian

# Codex
codex plugin marketplace add blockchainian/claude
codex plugin add intel@blockchainian
```

Start a new session after installation. Resolve script paths from the absolute
loaded `SKILL.md` directory in each shell call. Both hosts use the same scripts,
archives and existing secrets-manager store. Account
provisioning uses the secrets-manager skill in the Secrets plugin, available in both
hosts; installing Intel does not migrate account state. Download-book continues to
launch its own headed Chrome through its existing Playwright script.

## Skills

- **`transcribe`** — turn audio into plain-text words, transcribed
  locally with whisper (`mlx-whisper`, Apple Silicon). No cloud, no API key.
  Handles a finite file or URL (batch) and an ongoing **live stream** — a
  direct HLS/Icecast/RTMP URL, or a Twitch/YouTube/X Spaces live resolved with
  `streamlink`/`yt-dlp` — segmented and transcribed as it plays. Idempotent
  `setup.sh` auto-installs what's missing. A reusable audio-to-text step: any
  skill that has audio and needs its words calls this one.
- **`digest`** — turn a source into durable highlights, stored and searchable.
  Extracts an article's main body (trafilatura) or a podcast transcript, reads
  YouTube via subtitles, and for a page that only offers audio falls back to
  `transcribe`. A PDF (URL or local file) is highlighted chapter by chapter
  (its bookmarks; per page without them) and the highlights are typeset as a
  PDF in `translate`'s book format at the source's page size.
  A bare URL or file digests; `save` / `search` manage the store, with per-item
  take-aways.
- **`translate`** — a whole English EPUB → a Chinese PDF in the original's
  format: same cover image, page size and colors, chapter structure, running
  heads and folios, a regenerated clickable 目录 and flat bookmarks. Sections
  come from the EPUB's OPF spine and nav/ncx, keeping bold, emphasis,
  sub/superscripts and images. Each section is one `gpt-6-luna` call
  through `codex exec`, 20 in flight; headless Chrome typesets in Baskerville + Songti SC; pikepdf adds the
  cover and bookmarks. Resumable, per-section previews, plain-Markdown edits.
- **`analyze-appstore-reviews`** — a scraped App Store reviews JSON → a concise
  Chinese analysis doc with charts: likes, dislikes and feature requests ranked
  by frequency, every claim backed by a verbatim review.
- **`find-domain-names`** — brainstorm a brand name for a theme you give and
  return only the ones whose domain is registrable: short coined words (then
  metaphor words), checked live on `.xyz/.ai/.fun` via Namecheap's official API
  (`check.mjs`), same-name collisions against anything popular filtered out.
- **`download-book`** — search Anna's Archive for EPUB results, compare their
  download counts, and return fast and slow download links without fetching
  the book file.

- **`case-study`** — research one named creator in depth and typeset a sourced
  case study PDF in `digest`'s book format: starting point, the dated growth
  record from archive snapshots, methods, money, failures, and what can be
  copied. A workflow runs everything in parallel — the news lists fetched once,
  scouts, readers in batches, numbers from the archive in one batch, then a
  writer per chapter; sellers of courses and growth tools are never evidence.
  A separate writer then turns the chapters into a short book: a cover with the subject's name and linked accounts, an introduction,
  numbered chapters, no citations in the text, and figures shown as line
  charts, bar charts and tables instead of recited in sentences. One subject
  per run.

- **`fetch-x-mentions`** — archive X mentions and verify vendor tokens into stored ct0 pairs.
- **`fetch-x-posts`** — fetch X search posts with account rotation.
- **`fetch-x-user-posts`** — archive timelines from usernames or a roster file.
- **`analyze-x-mentions`** — clean, label and aggregate mention archives.
- **`analyze-x-user`** — analyze one user's posts and representative quotes.
- **`analyze-x-users`** — profile groups of users and build a local CRM.
- **`fetch-tiktok-mentions`** — archive TikTok videos and comments through ISP sessions.
- **`fetch-app-reviews`** — archive written App Store reviews across storefronts.

Archive paths are relative to the working directory; run from the repo root that owns the archive. X/TikTok accounts are logged in
through the secrets-manager skill in the Secrets plugin.

## Why they live together

`transcribe` and `digest` answer the same question — *what was actually said or written, and
what of it is worth keeping* — from different source shapes. `transcribe`
is the floor: it gets words out of sound. `digest` builds highlighting and a
searchable store on top, over whatever produced the words. New source kinds
(Twitch VODs, conference talks) slot in by reusing `transcribe` for the
audio leg and `digest` for the notes.

## Configuration

Intel uses `~/.config/intel/.env` for file-based request settings and API keys, shared by both hosts and all checkouts. Create the directory, copy this plugin’s `.env.example` there as `.env`, and fill in only the values needed by the skills you use. Each skill’s **Environment Variables** section lists its settings and source; the data root defaults to `~/.local/share/intel` and the output root to `~/Documents`. Account credentials and login sessions stay in Secrets Manager.

## Requirements

- `transcribe`: Apple Silicon; `setup.sh` installs `ffmpeg`,
  `streamlink`, `yt-dlp`, and a whisper runner (`mlx_whisper`/`uv`) via
  Homebrew. `curl` for URL downloads.
- `digest`: `curl`; `setup.sh` installs `uv` (runs the trafilatura article
  extractor and the PDF scripts), `yt-dlp` (YouTube subtitles) and `poppler`
  (PDFs); a highlights PDF needs Google Chrome.
- `translate`: `setup.sh` installs `poppler` and `uv` (for `render.py`; extract and translate are Node scripts); needs a logged-in
  `codex` CLI (ChatGPT plan, for gpt-6-luna) and Google Chrome.
- `find-domain-names`: Node.js 20.12+ and Namecheap API access with the calling IP whitelisted.
- `case-study`: Node.js 18+ (its own scripts), everything `digest` needs (it renders with `digest`'s PDF
  script), plus `curl`; a file of machine-tested tool commands is optional.
- `download-book`: Node.js 20.12+, Google Chrome, and `npm install` in the skill's
  `scripts/` dir (Playwright drives a headed Chrome window through the site's
  DDoS-Guard check; headless browsers get a captcha).

## Tests

```
node --test skills/transcribe/tests/*.mjs
uv run skills/digest/tests/test_fetch_source.py
node --test skills/digest/tests/*.mjs
uv run skills/digest/tests/test_pdf_highlights.py
node --test skills/translate/tests/*.mjs
uv run skills/translate/tests/test_render.py
uv run skills/translate/tests/test_lint_md.py
node --test skills/find-domain-names/tests/check.test.mjs
node --test skills/download-book/tests/*.mjs
node --test skills/case-study/tests/*.mjs
node --test skills/analyze-appstore-reviews/tests/*.mjs
```

`transcribe.test.mjs` covers the batch and live command shapes, the
chunk-readiness logic, platform resolution, and `setup.sh --check`; when
`ffmpeg` and a whisper runner are present it also runs a real end-to-end batch
and live transcription of a generated clip.

Install research dependencies in the installed skill directories: fetch-x-mentions/scripts,
fetch-app-reviews/scripts and fetch-tiktok-mentions/scripts. The X post and timeline
skills share the X client install. Each portable SKILL.md gives its skill-relative command; case-study retains Claude plugin-root commands.

Keep `.claude-plugin/plugin.json` and `.codex-plugin/plugin.json` versions equal.
