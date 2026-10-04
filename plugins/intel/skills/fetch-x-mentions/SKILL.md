---
name: fetch-x-mentions
description: Fetch every X/Twitter post that mentions an app over a date range into docs/intel/x/<slug>/tweets.jsonl — one authenticated search per UTC day, sharded across X accounts through the residential proxy, resumable. Use when asked to fetch / 抓 / 拉 an app's X mentions, extend an existing archive to today, or refill its gap days. NOT for a few posts on a search query without saving them (use fetch-x-posts), NOT for reading the archive (analyze-x-mentions, analyze-x-users) and NOT for one-off lookups (/x).
---

# Fetch X mentions

Run from the repo root.

```
node \
  "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts/fetch-x-mentions.mjs" \
  <slug> "<query>" [since] [until] [--daily-limit <n>] [--refill [<n>]]
```

- `slug` names the output dir `docs/intel/x/<slug>/`.
- `query` is X search syntax, e.g. `'(@alpha OR to:alpha OR "alpha app" OR alpha.family) -filter:nativeretweets'`.
  Keep to `@handle`, `to:handle`, `"phrase"` and domain terms; a bare brand word is mostly noise.
- `since`/`until` are UTC days, `until` exclusive; `since` defaults to 2025-09-20, `until` to today.
- `--daily-limit` caps tweets per day (default 1000). A day that hits it is truncated, newest first.
- Rerun the same command to resume: progress is per day in `tweets.out.json`.
- `--refill [n]` re-fetches gap days: 0 tweets, or cut at a page boundary below the limit.
  An empty page is X throttling, not the end of the day. `n` skips days already refilled more than `n` times.

## Output

- `tweets.jsonl`: one tweet per line (`id, author, text, created_at, likes, replies, lang, url, sources`),
  deduplicated by id, newest first once a run completes.
- `tweets.out.json`: `{ query, dailyLimit, days: { "YYYY-MM-DD": { count, oldest, refill } } }`.

## Config

Accounts come from the secrets-manager store (`~/.config/secrets-manager/secrets.sqlite`) — the single
source. Fill it with the secrets-manager skill (`secrets-manager import x` then the verify-x.mjs command below); each account is its
own rate bucket. `SECRETS_DB` overrides the store path.

`~/.config/intel/.env` holds only request config:

- `RESIDENTIAL_PROXY_URL`, or `X_PROXY_URLS` aligned to the accounts by row order.
- `X_SEARCH_QUERY_ID`, `X_BEARER`, `X_TID_*`: captured from x.com; re-extract when X answers 404 or the
  transaction-id check fails. The script comments say where each comes from.

## Failures

- `failed after N attempts` is the proxy, not a ban; rerun. A ban is `SearchTimeline 401/403` with a body.
- `Cannot find package 'undici'` in a fresh worktree:
  `npm install --prefix "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts"`.

## Test

```
node --test "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/tests/fetch-x-mentions.test.mjs"
```

Archive paths are relative to the working directory, run from the repo root that owns the archive.

Log the account in with the `secrets` plugin’s `secrets-manager login x`.

`~/.config/intel/.env` loads automatically without replacing existing environment values.
Keys: `X_BEARER`, `X_SEARCH_QUERY_ID`, `X_USER_QUERY_ID`, `X_USER_TWEETS_QID`,
`X_TID_VERIFICATION`, `X_TID_FRAME`, `X_TID_ROW`, `X_TID_INDICES`,
`RESIDENTIAL_PROXY_URL`, `X_PROXY_URLS`, `ISP_PROXY_URL`, `ISP_PROXY_COUNT`.
Only keys needed by this script are required. Missing required keys report this config path.

Setup: `npm install --prefix "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts"`.

Verify vendor tokens into stored ct0 pairs after importing X accounts:
```sh
node "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts/verify-x.mjs" [--select USER]... [--all] [--concurrency N]
```
Writes the existing secrets store; defaults to rows with a token but no ct0.
A rejected token is expired; a valid pair is active. Request config is ~/.config/intel/.env.
