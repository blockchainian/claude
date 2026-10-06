---
name: fetch-x-posts
description: Fetch a few X/Twitter posts that match any search query onto stdout as JSON lines, nothing saved — one authenticated SearchTimeline query on the X accounts through the residential proxy, the accounts rotated by one state shared across every process on the machine, so many can search at once. Use when asked to fetch / 抓 / 拉 / 搜 X posts for a search query, an account or a keyword, or when a research agent needs X search results. NOT for every post that mentions an app (use fetch-x-mentions), NOT for an account's whole timeline (use fetch-x-user-posts) and NOT for reading an archive (analyze-x-mentions, analyze-x-user).
---

# Fetch X posts

## Runtime and paths

Works in Claude Code and Codex. Resolve `SKILL_DIR` from the absolute directory of
this loaded `SKILL.md`, not the working directory or a host-specific environment variable:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

Repeat this assignment and any `S`, `T` or `U` assignments used below in every shell call;
shell variables may not persist between calls. If the loaded path is unavailable, stop
and report it. Keep the full intel plugin installed: sibling skills share scripts.
Run archive commands from the repository that owns the archive; configuration and
account stores are shared between hosts and are not migrated by installing intel.

For finite long-running commands, choose a deadline before launch and retain the process
handle and output. In Claude Code use `run_in_background` and its completion notification;
in Codex use the shell tool's process/session handle and wait for completion. Subagents
must await their own commands before returning. Do not repeatedly poll logs or assume a
background completion wakes either host. On timeout, preserve diagnostics and report the
process state before retrying. Use the current host's image/file tools to inspect artifacts.

Fetch the posts one search returns and print them — as opposed to `fetch-x-mentions` and
`fetch-x-user-posts`, which fetch everything over a date range and save it under `docs/intel/x/`.
Give it any X search query; it prints a small number of posts and writes no file. The request
(x-client-transaction-id, the account list, the SearchTimeline url, the proxy) is imported from
`fetch-x-mentions`, with request settings loaded from this script's own directory.

Run from the repo root.

```
node \
  "$SKILL_DIR/scripts/fetch-x-posts.mjs" \
  "<query>" [--limit <n>] [--latest|--top]
```

- `query` is X search syntax in one quoted argument, passed as written, so operators work:
  `"from:zachxbt min_faves:5000"`, `"Kobeissi Letter since:2026-01-01_00:00:00_UTC"`, `"to:alpha filter:replies"`.
- `--limit` is how many posts to print (default 40). A request returns 20 and takes 1 to 2 seconds.
- `--latest` (the default) is the chronological view, `--top` X's ranked one.
- From another directory, call the script by its full path. It loads `~/.config/intel/.env` automatically.

## Output

On stdout, one post per line (`id, url, created_at, user, name, text, likes, retweets, replies,
views, quoted, in_reply_to`), in X's order, deduplicated by id.

- `created_at` is UTC. `text` is the whole post: the long-post body, and for a retweet the
  original's text. `quoted` is the quoted post's url or null, `in_reply_to` the parent's id or null.
- Exit 0 with no lines only when three accounts in a row answered an empty page.
- Any failure exits 1; the last stderr line is `fetch-x-posts: <reason>`. Posts found before the
  failure are still printed, so read stdout before the exit code.
- stderr may carry note lines first: `@user answered 403, marked bad`, and `N of M accounts usable`
  when fewer than 10 are.

## Account rotation

Many processes may run at once. They share `~/.cache/case-study-limits/fetch-x-posts.sqlite`
(`accounts`: when each may start its next request and until when it is paused; `requests`: a log
of every request):

- each request goes to the account that is not paused and was used longest ago;
- two requests on one account start at least 3.2 seconds apart;
- a 429 pauses the account until `x-rate-limit-reset` (or 15 minutes) and the next account takes over;
- an empty page is as often X throttling as a true end (the reason `fetch-x-mentions` distinguishes
  gap days), so the same page is asked on the next account, up to three, before it counts as the end;
- a 401/403 marks the account bad and skips it for 6 hours. The secrets store is never written.

When every account is paused the run exits 1 and names the earliest reset. One run waits at most
60 seconds in total for accounts; one request is cut off after 30 seconds.

`fetch-x-mentions` and `fetch-x-user-posts` use the same accounts without this state. A full
fetch running at the same time can push an account into a 429, which this script then pauses.

## Environment Variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env`, then fill in only the values needed by the skills you use. The CLI loads that file without replacing variables already exported in the shell.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `X_BEARER` | X web-client bearer token | Yes | `~/.config/intel/.env` |
| `X_SEARCH_QUERY_ID` | SearchTimeline operation ID | Yes | `~/.config/intel/.env` |
| `X_TID_VERIFICATION` | Site-verification value used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_FRAME` | Animation frame data used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_ROW` | Animation row index used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_INDICES` | Key-byte indices used to sign requests | Yes | `~/.config/intel/.env` |
| `RESIDENTIAL_PROXY_URL` | Default residential proxy | One proxy source required | `~/.config/intel/.env` |
| `X_PROXY_URLS` | Comma-separated proxies aligned to account row order | Alternative to the default proxy | `~/.config/intel/.env` |
| `HTTPS_PROXY` | Default proxy when RESIDENTIAL_PROXY_URL is unset | Alternative to RESIDENTIAL_PROXY_URL | `~/.config/intel/.env` |
| `SECRETS_MANAGER_STATE_PATH` | Account-store directory; default ~/.config/secrets-manager | No | `~/.config/intel/.env` |
| `SECRETS_DB` | SQLite account-store path; overrides the directory setting | No | `~/.config/intel/.env` |

Account credentials (`auth_token`, `ct0`) stay in the existing Secrets Manager store, normally `~/.config/secrets-manager/secrets.sqlite`; do not copy them into `.env`. Capture the X web-client and signing values from x.com; refresh them when its web bundle changes.

Requires Node.js 22.13+ for `node:sqlite`.

## Failures

- `@user answered 403, marked bad`: the stored ct0 no longer matches the session (X error 353) or
  the account is locked. Run `intel verify-x.mjs --all` with the secrets-manager skill, then put the accounts back:
  `sqlite3 ~/.cache/case-study-limits/fetch-x-posts.sqlite "update accounts set paused_until=0, bad=null"`.
- `all N accounts are paused`: wait for the reset it names.
- `failed after N attempts` is the proxy, not a ban; rerun.
- `SearchTimeline 400` mentioning the operation/features means X redeployed; update
  `X_SEARCH_QUERY_ID` / `FEATURES` in `fetch-x-mentions`.
- `Cannot find package 'undici'` in a fresh worktree:
  `npm install --prefix "$SKILL_DIR/../fetch-x-mentions/scripts"`.

## Test

```
node --test "$SKILL_DIR/tests/fetch-x-posts.test.mjs"
```

Archive paths are relative to the working directory, run from the repo root that owns the archive.

Log the account in with the `secrets` plugin’s `secrets-manager login x`.

Setup shared X client: `npm install --prefix "$SKILL_DIR/../fetch-x-mentions/scripts"`.

## Shared account prerequisite

Use the existing secrets-manager CLI and store to provision or log in accounts.
It need not be installed as a Codex plugin to run its CLI. If the CLI, required
account, proxy or browser profile is missing, report the prerequisite; do not
create a second store or switch to a host browser profile.
