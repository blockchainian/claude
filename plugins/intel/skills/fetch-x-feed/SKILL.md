---
name: fetch-x-feed
description: >-
  Fetch the logged-in X account's Following timeline or a trending topic into a dated archive, measure it and save its digest. Use for a feed TLDR, what the feed is discussing, a time-window pull or a trending topic page. NOT for an account's posts (use fetch-x-user-posts), NOT for keyword searches or threads (use fetch-x-posts, with a conversation_id query for a thread), and NOT for account actions (use manage-x-account).
---

# fetch-x-feed

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from sibling skills, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin's `.env.example` to the shared intel `.env` configuration file. The scripts
load it without replacing variables already exported in the shell. Resolve the configuration
location and storage roots through `fetch-x-mentions/scripts/env.mjs`.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_STATE_DIR` | State root; feed archives and digest history live under `x/feed/` | No | Shared intel `.env` |

Trending topics use the sibling `fetch-x-posts` script. Its **Environment variables** and
**Setup** sections cover the additional search configuration and account store; do not copy
credentials into this skill or its archives.

## Setup

Use Node.js 22.13+ and have `opencli` installed on `PATH`. Ask the user to run
`opencli twitter login` if needed. Check the logged-in account with
`opencli twitter whoami -f json` before fetching; if it cannot confirm the account, stop.
Do not attempt a login yourself.

## Fetch, measure and read

```sh
node "$SKILL_DIR/scripts/fetch-x-feed.mjs" tldr --window 24h
node "$SKILL_DIR/scripts/measure-x-feed.mjs" "<archive>" --top 25 --terms topic,project
```

Run the measurement before reading posts. It reports reach tiers, bio personas, Chinese-language
share, tickers, engagement percentiles, top posts and views per reply. Use the surfaced posts to
answer the question. Profile follower counts have been observed as zero in opencli; use measured
views per reply rather than treating those counts as real reach.

Read the fetch summary before making volume claims. Timeline has no cursor: each pull scrolls
from the top and bottoms out near 850 items. A 24-hour request has returned roughly 13 dense
hours plus a sparse tail. Quote `dense_from` and `dense_share`, alongside the actual oldest and
newest timestamps, rather than treating the requested window as covered. Do not chart hourly
volume across uncovered hours. Small pulls may contain too few rows to measure a dense band.

For a trending topic, pass its page URL in place of `tldr`. The script resolves the page's
headline, searches it through `fetch-x-posts`, and records both the headline search and, when
thin, the distinctive-word search. A topic page is not a thread; a thin result does not describe
the whole trend.

For reception or audience questions, sample conversations across large and medium posts,
languages and relevant subjects. Fetch each with `fetch-x-posts "conversation_id:<id>"`;
this skill has no thread mode. Optional `replies.json` in an archive can be measured alongside
its feed. Before growth advice, use `fetch-x-user-posts` to establish the user's posting cadence,
voice and last-post date. A plain feed TLDR needs neither extra step.

Give short answers in chat. Create a report only when requested or useful as a lasting reference,
and put the coverage limits near the top.

## Save the digest

```sh
node "$SKILL_DIR/scripts/save-x-digest.mjs" --archive "<archive>" --text "<exact digest already shown>"
```

Save the exact digest already shown in this session, without fetching again or rewriting it.
If no timeline ran this session, fetch it first. Omit `--archive` to use the newest timeline
archive, or pipe the exact digest on stdin instead of using `--text`. The digest is stored as
`tldr.md`; `x/feed/tldr-history.jsonl` keeps `saved_at`, `archive` and `digest` for cross-day reading.

## Command reference

| Script | Arguments |
| --- | --- |
| `fetch-x-feed.mjs` | `tldr` or a trending topic URL, `--window 24h`, optional `--limit <n>` |
| `measure-x-feed.mjs` | `<archive>`, optional `--top 25` and `--terms word,word` |
| `save-x-digest.mjs` | Optional `--archive <archive>` and `--text <digest>`; otherwise stdin |

Windows accept minutes, hours, days or weeks (`90m`, `24h`, `7d`, `2w`), or inclusive UTC
calendar dates (`2026-09-01..2026-09-07`). Timeline sizes itself from a 150-row probe unless
`--limit` is supplied; an explicit timeline limit is bounded at the observed 850-row scroll cap.
Topic search defaults to 60 rows per query. Archives are dated `x-tldr-*` or `x-topic-*`
directories under the feed state directory, with `feed.json`, untouched `raw-*.json` pulls and a
`README.md` coverage manifest. Repeating a mode and topic on the same date updates that archive.
The fetch command prints one JSON summary line without account handles; measurements print JSON.

## Failures and limits

Observed opencli bursts tolerated roughly 1,600 timeline rows without a 429; account-post and
search pulls were stricter at roughly 400–600. These are observations, not guaranteed quotas;
trending searches use `fetch-x-posts`' own limits. Long pulls run in the background using the
Bash tool's `run_in_background` and timeout field, never shell `timeout`. Wait for process
completion rather than polling file counts, and do not read a file still being written.

A failed deep pull leaves the probe archive and a coverage note. Other fetch failures exit
nonzero; empty filtered captures have zero posts and a note. A missing topic title means the
page may be stale; do not substitute its numeric ID as a query. Search failures report through
the sibling script; fix its prerequisites instead of switching to opencli search.

## Tests

From the marketplace root:

```sh
node --test plugins/intel/skills/fetch-x-feed/tests/*.test.mjs
```
