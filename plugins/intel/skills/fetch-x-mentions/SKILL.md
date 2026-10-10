---
name: fetch-x-mentions
description: Fetch every X/Twitter post that mentions an app over a date range into a resumable per-app archive (tweets.jsonl) under the intel state root — one authenticated search per UTC day, sharded across X accounts through the residential proxy. Use when asked to fetch / 抓 / 拉 an app's X mentions, extend an existing archive to today, or refill its gap days. NOT for a few posts on a search query without saving them (use fetch-x-posts), NOT for reading the archive (analyze-x-mentions, analyze-x-users) and NOT for one-off lookups (/x).
---

# Fetch X mentions

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env` and fill in only the values the
skills you use need. The scripts load that file without replacing variables already exported in the
shell.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `X_BEARER_TOKEN` | X web-client bearer token | Yes | `~/.config/intel/.env` |
| `X_SEARCH_QUERY_ID` | SearchTimeline operation ID | Yes | `~/.config/intel/.env` |
| `X_TID_VERIFICATION` | Site-verification value used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_FRAME` | Animation frame data used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_ROW` | Animation row index used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_INDICES` | Key-byte indices used to sign requests | Yes | `~/.config/intel/.env` |
| `RESIDENTIAL_PROXY_URL` | Default residential proxy; `verify-x.mjs` also requires it | Yes | `~/.config/intel/.env` |
| `INTEL_STATE_DIR` | State root; default `~/.local/state/intel`, with X archives under `x/` | No | `~/.config/intel/.env` |
| `SECRETS_STATE_DIR` | Account-store directory; default `~/.local/state/secrets-manager` | No | `~/.config/intel/.env` |

Capture the X web-client and signing values from a signed-in X web session, and capture them again
when X changes its web bundle.

## Setup

Install the script dependencies once, and again in a fresh worktree:

```sh
npm install --prefix "$SKILL_DIR/scripts"
```

The X accounts come from the existing secrets-manager store, and their credentials (`auth_token`,
`ct0`) stay there; never copy them into `.env`. Use burner accounts only, never the user’s own,
because every `active` row is used; each account has its own rate bucket. Import them with the
`secrets` plugin’s `secrets-manager import x`, or log one in with `secrets-manager login x`. The
secrets-manager CLI runs without being installed as a Codex plugin. If the CLI, an account, the
proxy or a browser profile is missing, report that prerequisite; never create a second store or
switch to a host browser profile.

After importing accounts, turn their tokens into stored `ct0` pairs:

```sh
node "$SKILL_DIR/scripts/verify-x.mjs" [--select USER]... [--all] [--concurrency N]
```

It writes the existing store and by default checks the rows that have a token but no `ct0`. A
valid pair marks the account `active`; a rejected token marks it `expired`.

## Run

Run from any working directory; the data lives under the state root. A run over many days takes a
long time, so run it in the background.

```sh
node "$SKILL_DIR/scripts/fetch-x-mentions.mjs" \
  <slug> "<query>" [since] [until] [--daily-limit <n>] [--refill [<n>]]
```

- `slug` names the archive directory, `x/<slug>/` under the state root.
- `query` is X search syntax, for example
  `'(@alpha OR to:alpha OR "alpha app" OR alpha.family) -filter:nativeretweets'`. Keep to
  `@handle`, `to:handle`, `"phrase"` and domain terms, because a bare brand word is mostly noise.
- `since` and `until` are UTC days, with `until` exclusive; `since` defaults to 2025-09-20 and
  `until` to today.
- `--daily-limit` caps the tweets per day (default 1000). A day that hits the cap is truncated,
  keeping the newest tweets.
- `--refill [n]` re-fetches gap days: days with 0 tweets, or cut at a page boundary below the
  limit. An empty page is X throttling, not the end of the day. With `n`, days already refilled
  more than `n` times are skipped.

To resume, rerun the same command; progress is kept per day in `tweets.out.json`.

## Output

- `tweets.jsonl` holds one tweet per line (`id, author, text, created_at, likes, replies, lang,
  url, sources`), deduplicated by id and newest first once a run completes.
- `tweets.out.json` is
  `{ query, dailyLimit, days: { "YYYY-MM-DD": { count, oldest, refill } } }`.

## Failures

`failed after N attempts` is the proxy, not a ban, so rerun. A ban shows as `SearchTimeline
401/403` with a response body. `Cannot find package 'undici'` means the dependencies are missing;
run the install from Setup.

## Tests

```sh
node --test "$SKILL_DIR/tests/fetch-x-mentions.test.mjs"
```
