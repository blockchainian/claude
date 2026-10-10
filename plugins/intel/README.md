# intel

Research skills for Claude Code and Codex. They turn long-form sources (articles, podcasts, videos,
live streams, PDFs and whole books) into transcripts, highlights and translations, archive and
analyze what people say about an app on X, TikTok and the App Store, research creators in depth,
find book downloads and check which brand domains are still registrable. They also archive and
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
| `translate` | Translate an English EPUB into a Chinese PDF in the original's format. |
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

Each skill's `SKILL.md` covers its usage, setup and requirements.

## Tests

Run `npm run test:intel` from the marketplace root.
