---
name: fetch-x-posts
description: Fetch a few X/Twitter posts that match any search query onto stdout as JSON lines, nothing saved, through the X accounts of the secrets-manager store; many processes can search at once. Use when asked to fetch / 抓 / 拉 / 搜 X posts for a search query, an account or a keyword, or when a research agent needs X search results. NOT for every post that mentions an app (use fetch-x-mentions), NOT for an account's whole timeline (use fetch-x-user-posts) and NOT for reading an archive (analyze-x-mentions, analyze-x-user).
---

# Fetch X posts

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
script imports from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env` and fill in the values below. The
script loads that file without replacing variables already exported in the shell.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `X_BEARER_TOKEN` | X web-client bearer token | Yes | `~/.config/intel/.env` |
| `X_SEARCH_QUERY_ID` | SearchTimeline operation ID | Yes | `~/.config/intel/.env` |
| `X_TID_VERIFICATION` | Site-verification value used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_FRAME` | Animation frame data used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_ROW` | Animation row index used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_INDICES` | Key-byte indices used to sign requests | Yes | `~/.config/intel/.env` |
| `RESIDENTIAL_PROXY_URL` | Default residential proxy | Yes | `~/.config/intel/.env` |
| `INTEL_STATE_DIR` | State root; the account-rotation state lives under `limits/`; default `~/.local/state/intel` | No | `~/.config/intel/.env` |
| `SECRETS_STATE_DIR` | Account-store directory; default `~/.local/state/secrets-manager` | No | `~/.config/intel/.env` |

The web-client and signing values are captured from x.com; refresh them when its web bundle
changes. Account credentials stay in the secrets-manager store and never go into `.env`.

## Setup

The script needs Node.js 22.13+. Install the shared X client once:

```sh
npm install --prefix "$SKILL_DIR/../fetch-x-mentions/scripts"
```

Log the X accounts in with the `secrets` plugin's `secrets-manager login x`. Its CLI runs without
being installed as a Codex plugin. If the CLI, an account, the proxy or a browser profile is
missing, report that prerequisite; never create a second store or switch to a host browser profile.

## Run

One run sends one X search and prints the posts it returns; nothing is written to disk.

```sh
node "$SKILL_DIR/scripts/fetch-x-posts.mjs" "<query>" [--limit <n>] [--latest|--top]
```

- The query is X search syntax in one quoted argument and goes to X as written, so operators work:
  `"from:zachxbt min_faves:5000"`, `"Kobeissi Letter since:2026-01-01_00:00:00_UTC"`,
  `"to:alpha filter:replies"`.
- `--limit` sets how many posts to print (default 40). Each request returns 20 posts and takes 1 to
  2 seconds.
- `--latest`, the default, is the chronological view; `--top` is X's ranked one.

Search does not return an account's profile (bio, links, follower counts); get that with
`curl -s https://api.fxtwitter.com/<handle>`.

## Output

Each stdout line is one JSON object per post, in X's order, with `id, url, created_at, user, name,
text, likes, retweets, replies, views, quoted, in_reply_to`. `created_at` is UTC and `text` is the
whole post, for a retweet the original's text. `quoted` is the quoted post's url and `in_reply_to`
the parent's id, each null when absent.

Exit 0 with no lines means the search has no results. Any failure exits 1 with a last stderr line
`fetch-x-posts: <reason>`, but the posts found before it are still printed, so read stdout before
the exit code. Earlier stderr lines are notes: `@user answered 403, marked bad`, and
`N of M accounts usable` when fewer than 10 are.

## Failures

- `@user answered 403, marked bad`: the account's stored session no longer matches or the account
  is locked. Re-derive the session pairs with
  `node "$SKILL_DIR/../fetch-x-mentions/scripts/verify-x.mjs" --all`, then put the accounts back
  into rotation with
  `sqlite3 "${INTEL_STATE_DIR:-$HOME/.local/state/intel}/limits/fetch-x-posts.sqlite" "update accounts set paused_until=0, bad=null"`.
- `all N accounts are paused`: X rate-limited every account; wait for the reset time it names.
- `failed after N attempts` is the proxy, not a ban; rerun.
- `SearchTimeline 400` mentioning the operation or features means X redeployed; update
  `X_SEARCH_QUERY_ID` in `~/.config/intel/.env` and `FEATURES` in `fetch-x-mentions.mjs`.
- `Cannot find package 'undici'` in a fresh worktree means the Setup install is missing; run it.

The full fetches, `fetch-x-mentions` and `fetch-x-user-posts`, use the same accounts, so one running
at the same time can rate-limit accounts this search wanted.

## Tests

```sh
node --test "$SKILL_DIR/tests/fetch-x-posts.test.mjs"
```
