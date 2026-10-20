# intel

Research skills for Claude Code and Codex. They turn long-form sources (articles, podcasts, videos,
live streams, PDFs and whole books) into transcripts, highlights and translations, archive and
analyze what people say about an app on X, TikTok and the App Store, research creators in depth,
find book downloads, check which brand domains are still registrable and give minified
JavaScript identifiers meaningful names. They also archive and
measure the X Following feed and trending topics, save feed digests, and preview or perform
X account actions through opencli.

## Install

```sh
# Claude Code
/plugin marketplace add blockchainian/claude
/plugin install intel@blockchainian

# Codex
codex plugin marketplace add blockchainian/claude
codex plugin add intel@blockchainian
```

Start a new session after installing or updating. Both hosts load the same skills and scripts,
except `case-study`, which runs as a Claude Code workflow and is not available in Codex. Skills
that sign in to X or TikTok use the accounts stored by the `secrets-manager` skill of the secrets
plugin. Feed reads and account actions instead use the user’s logged-in opencli account.

## Configuration

Settings and API keys go in `~/.config/intel/.env`, shared by both hosts and all checkouts. Copy
this plugin's `.env.example` there and fill in only what the skills you use need; each skill's
**Environment variables** section lists its settings. Files land under three roots:

- `INTEL_OUTPUT_DIR` (default `~/Documents`): finished outputs.
- `INTEL_STATE_DIR` (default `~/.local/state/intel`): fetched archives, labels, work directories
  and rate limits that must survive.
- `INTEL_DATA_DIR` (default `~/.local/share/intel`): browser profiles and scratch files that can be
  deleted.

Scripts resolve these roots through `skills/fetch-x-mentions/scripts/env.mjs`, never a hard-coded
path.

## Skills

| Skill | Purpose |
|---|---|
| `transcribe` | Transcribe audio or a live stream locally with whisper. |
| `digest` | Turn an article, transcript, video or PDF into searchable highlights. |
| `translate-epub` | Translate an English EPUB into a Chinese PDF in the original's format. |
| `case-study` | Write a sourced case study of one creator as a short PDF book. |
| `download-book` | Find EPUB downloads on Anna's Archive and fetch the chosen book. |
| `find-domain-names` | Brainstorm brand names and keep those with a registrable domain. |
| `fetch-app-reviews` | Archive an iOS app's written reviews across all App Store storefronts. |
| `analyze-appstore-reviews` | Rank an app's likes, dislikes and feature requests from its reviews. |
| `fetch-x-mentions` | Archive every X post mentioning an app over a date range. |
| `fetch-x-posts` | Print a few X posts for any search query. |
| `fetch-x-user-posts` | Archive X accounts' own posts and replies. |
| `fetch-x-feed` | Archive and measure the Following feed or a trending topic and save its digest. |
| `manage-x-account` | Preview and perform X account actions, individually or in batches. |
| `analyze-x-mentions` | Analyze an app's reception on X from its mentions archive. |
| `analyze-x-users` | Profile the accounts that mention an app. |
| `analyze-x-user` | Profile one X account from its own timeline. |
| `fetch-tiktok-mentions` | Archive a brand's TikTok videos, comments and video files. |
| `unminify` | Rename short JavaScript identifiers with model-suggested, scope-checked names. |

Each skill's `SKILL.md` covers its usage, setup and requirements.

Unminify plans and packs batches within each file and stage using `--context-tokens`
(default 100000). The budget counts instructions, prompt and rendered context with
`js-tiktoken` 1.0.21 (`o200k_base`); there is no name-count cap. Oversized contexts are
shortened; an irreducible batch that still cannot fit fails before sending. Top-level
naming finishes before function naming, and journals retain original batch IDs.
Every run, including an interrupted run, updates import names in journal-completed files.
Unfinished files supply their current export names without being rewritten, preserving resume.

Unminify appends safe HTTP-attempt timing, queue/concurrency, retry and session metadata to
`.unminify/requests.jsonl`, and run/file/phase timings, local-step totals, 429 pauses and final
performance summaries to `.unminify/performance.jsonl` in the target Git root. Telemetry
contains no credentials or source text. Analyze the latest run locally with
`node plugins/intel/skills/unminify/scripts/perf.mjs <target-repo>`; optional `--run-id <id>`,
`--window-seconds 10` and `--slow-files 10` select the run, window size and slow-file count.
The JSON report and run summary include model HTTP-attempt count and median/mean names per
request (retries included); attempt logs list batch IDs, name count and `estimated_input_tokens`.
Summary and perf reports compare total `estimated_input_tokens` with server `input_tokens`
(including retries). Estimates count input text; server framing and schema accounting may differ.
The report also includes windowed throughput, concurrency, latency and 429 counts, phase
elapsed times and slow files. Summary latency/throughput include refresh HTTP attempts;
local-step times include nested work and phase totals can overlap.


## Tests

Run `npm run test:intel` from the marketplace root. The unminify real-model test is skipped
unless `UNMINIFY_E2E=1`. Run its authored small JavaScript modules with the current Codex login:

```sh
npm run test:intel:unminify:e2e
```

It checks syntax checkpoints, binding safety, preserved exports, renamed imports and execution.
To inspect a real-model run on your own sample files, use:

```sh
node plugins/intel/skills/unminify/scripts/e2e.mjs <sample.js>...
```
