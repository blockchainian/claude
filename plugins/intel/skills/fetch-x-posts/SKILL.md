---
name: fetch-x-posts
description: Fetch a few X/Twitter posts that match any search query onto stdout as JSON lines, nothing saved, through the X accounts of the secrets-manager store; many processes can search at once. Use when asked to fetch / 抓 / 拉 / 搜 X posts for a search query, an account or a keyword, or when a research agent needs X search results. NOT for every post that mentions an app (use fetch-x-mentions), NOT for an account's whole timeline (use fetch-x-user-posts) and NOT for reading an archive (analyze-x-mentions, analyze-x-user).
---

# Fetch X posts

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

Run one X search and print the posts it returns; nothing is written to disk. For an account's
profile (bio, links, follower counts), which search does not return, use
`curl -s https://api.fxtwitter.com/<handle>`.

```
node \
  "$SKILL_DIR/scripts/fetch-x-posts.mjs" \
  "<query>" [--limit <n>] [--latest|--top]
```

- `query` is X search syntax in one quoted argument, passed as written, so operators work:
  `"from:zachxbt min_faves:5000"`, `"Kobeissi Letter since:2026-01-01_00:00:00_UTC"`, `"to:alpha filter:replies"`.
- `--limit` is how many posts to print (default 40). A request returns 20 and takes 1 to 2 seconds.
- `--latest` (the default) is the chronological view, `--top` X's ranked one.

## Output

stdout carries one JSON object per post (`id, url, created_at, user, name, text, likes, retweets,
replies, views, quoted, in_reply_to`), in X's order. `created_at` is UTC, `text` is the whole post
(for a retweet, the original's text), `quoted` is the quoted post's url or null and `in_reply_to`
the parent's id or null.

An exit 0 with no lines means the search has no results. Any failure exits 1 with a last stderr
line `fetch-x-posts: <reason>`, but the posts found before it are still printed, so read stdout
before the exit code. Earlier stderr lines are notes: `@user answered 403, marked bad`, and
`N of M accounts usable` when fewer than 10 are.

## Failures

- `@user answered 403, marked bad`: the stored ct0 no longer matches the session or the account is
  locked. Re-derive the ct0 pairs with `node "$SKILL_DIR/../fetch-x-mentions/scripts/verify-x.mjs" --all`,
  then put the accounts back into rotation:
  `sqlite3 ~/.local/state/intel/limits/fetch-x-posts.sqlite "update accounts set paused_until=0, bad=null"`.
- `all N accounts are paused`: X rate-limited every account; wait for the reset time it names.
- `failed after N attempts` is the proxy, not a ban; rerun.
- `SearchTimeline 400` mentioning the operation or features means X redeployed; update
  `X_SEARCH_QUERY_ID` in `~/.config/intel/.env` and `FEATURES` in `fetch-x-mentions.mjs`.
- `Cannot find package 'undici'` in a fresh worktree:
  `npm install --prefix "$SKILL_DIR/../fetch-x-mentions/scripts"`.

Full fetches (`fetch-x-mentions`, `fetch-x-user-posts`) share the same accounts, so one running at
the same time can rate-limit accounts this search wanted.

## Setup

Requires Node.js 22.13+. Install the shared X client with
`npm install --prefix "$SKILL_DIR/../fetch-x-mentions/scripts"`, and log the X accounts in with the
`secrets` plugin's `secrets-manager login x`.

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
| `INTEL_STATE_DIR` | State root; default ~/.local/state/intel, with the account-rotation state under limits/ | No | `~/.config/intel/.env` |
| `SECRETS_STATE_DIR` | Account-store directory; default ~/.local/state/secrets-manager | No | `~/.config/intel/.env` |

Account credentials stay in the secrets-manager store; do not copy them into `.env`. Capture the
web-client and signing values from x.com, and refresh them when its web bundle changes.

## Test

```
node --test "$SKILL_DIR/tests/fetch-x-posts.test.mjs"
```

## Shared account prerequisite

Use the existing secrets-manager CLI and store to provision or log in accounts.
It need not be installed as a Codex plugin to run its CLI. If the CLI, required
account, proxy or browser profile is missing, report the prerequisite; do not
create a second store or switch to a host browser profile.
