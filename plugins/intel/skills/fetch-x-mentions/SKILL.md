---
name: fetch-x-mentions
description: Fetch every X/Twitter post that mentions an app over a date range into ~/.local/state/intel/x/<slug>/tweets.jsonl — one authenticated search per UTC day, sharded across X accounts through the residential proxy, resumable. Use when asked to fetch / 抓 / 拉 an app's X mentions, extend an existing archive to today, or refill its gap days. NOT for a few posts on a search query without saving them (use fetch-x-posts), NOT for reading the archive (analyze-x-mentions, analyze-x-users) and NOT for one-off lookups (/x).
---

# Fetch X mentions

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

Run from any working directory; data paths are under `INTEL_STATE_DIR`.

```
node \
  "$SKILL_DIR/scripts/fetch-x-mentions.mjs" \
  <slug> "<query>" [since] [until] [--daily-limit <n>] [--refill [<n>]]
```

- `slug` names the output dir `~/.local/state/intel/x/<slug>/`.
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

## Environment variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env`, then fill in only the values needed by the skills you use. The CLI loads that file without replacing variables already exported in the shell.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `X_BEARER_TOKEN` | X web-client bearer token | Yes | `~/.config/intel/.env` |
| `X_SEARCH_QUERY_ID` | SearchTimeline operation ID | Yes | `~/.config/intel/.env` |
| `X_TID_VERIFICATION` | Site-verification value used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_FRAME` | Animation frame data used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_ROW` | Animation row index used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_INDICES` | Key-byte indices used to sign requests | Yes | `~/.config/intel/.env` |
| `RESIDENTIAL_PROXY_URL` | Default residential proxy | Yes | `~/.config/intel/.env` |
| `INTEL_STATE_DIR` | State root; default ~/.local/state/intel, with X archives under x/ | No | `~/.config/intel/.env` |
| `SECRETS_STATE_DIR` | Account-store directory; default ~/.local/state/secrets-manager | No | `~/.config/intel/.env` |

Account credentials (`auth_token`, `ct0`) stay in the existing Secrets Manager store, normally `~/.local/state/secrets-manager/secrets.sqlite`; do not copy them into `.env`. Capture the X web-client and signing values from x.com; refresh them when its web bundle changes.

Provision burner accounts only (every `active` row is used; never the user's own) with `secrets-manager import x`, then run `verify-x.mjs` below; each account has its own rate bucket.

`verify-x.mjs` specifically requires `RESIDENTIAL_PROXY_URL` when verifying tokens.

## Failures

- `failed after N attempts` is the proxy, not a ban; rerun. A ban is `SearchTimeline 401/403` with a body.
- `Cannot find package 'undici'` in a fresh worktree:
  `npm install --prefix "$SKILL_DIR/scripts"`.

## Test

```
node --test "$SKILL_DIR/tests/fetch-x-mentions.test.mjs"
```

Archive paths below show the default Intel state root; use the configured `INTEL_STATE_DIR` when set.

Log the account in with the `secrets` plugin’s `secrets-manager login x`.

Setup: `npm install --prefix "$SKILL_DIR/scripts"`.

Verify vendor tokens into stored ct0 pairs after importing X accounts:
```sh
node "$SKILL_DIR/scripts/verify-x.mjs" [--select USER]... [--all] [--concurrency N]
```
Writes the existing secrets store; defaults to rows with a token but no ct0.
A rejected token is expired; a valid pair is active. Request config is ~/.config/intel/.env.

## Shared account prerequisite

Use the existing secrets-manager CLI and store to provision or log in accounts.
It need not be installed as a Codex plugin to run its CLI. If the CLI, required
account, proxy or browser profile is missing, report the prerequisite; do not
create a second store or switch to a host browser profile.
