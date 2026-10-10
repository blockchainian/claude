---
name: fetch-x-user-posts
description: Fetch an X/Twitter account's own posts and replies over a date range into ~/.local/state/intel/x/kols/<user>/tweets.jsonl and replies.jsonl — the authenticated SearchTimeline `from:<user>` paged chronologically, sharded across X accounts through the residential proxy, resumable, for a batch of usernames at once. Use when asked to fetch / 抓 / 拉 one or many accounts' own timelines (e.g. a KOL roster's past-year posts). NOT for posts that mention an app (use fetch-x-mentions), NOT for a few of an account's posts without saving them (use fetch-x-posts) and NOT for reading the archive (analyze-x-user).
---

# Fetch X users

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

Fetch what accounts post themselves — their own tweets and their replies — as opposed to
`fetch-x-mentions`, which fetches what everyone says about an app. Give it a batch of screen
names; it writes one folder per user. The auth core (x-client-transaction-id, the account list,
the SearchTimeline request, page parsing, retry/quota handling, cross-account draining) is
imported from `fetch-x-mentions`, with request settings loaded from this script's own directory.

Run from any working directory; data paths are under `INTEL_STATE_DIR`.

```
node \
  "$SKILL_DIR/scripts/fetch-x-user-posts.mjs" \
  <user> [<user> ...] [--file <path>] [--since YYYY-MM-DD] [--until YYYY-MM-DD] \
  [--max-pages <n>] [--max-tries <n>]
```

- `<user>` is a bare screen name (a leading `@`, an `x.com/…` URL, or a trailing path are
  stripped; invalid names are dropped). Pass as many as you like.
- `--file` adds a newline list or a JSON object whose values are username arrays.
  Arguments and files are merged, validated and deduplicated.
- `--since`/`--until` are UTC days, `until` exclusive. `--since` defaults to one year before
  `--until`; `--until` defaults to today.
- `--max-pages` caps each **attempt** at `n` pages of 20 (default 500 = 10 000 posts). A stream
  longer than one attempt continues across attempts (see below), so this is a per-attempt guard,
  not a total cap.
- `--max-tries` is how many attempts an incomplete stream gets before it is accepted as-is
  (default 3; use 5 for a big overnight batch). Each account's SearchTimeline quota is ~50 pages
  per 15 min, so a very active account's year takes several attempts on rotating accounts.

## Batch example (a whole KOL roster, past year)

```
node \
  "$SKILL_DIR/scripts/fetch-x-user-posts.mjs" \
  --file docs/intel/kols/2026-09-24.json --since 2025-09-24 --until 2026-09-24
```

Every X account has its own rate bucket, so N accounts ≈ N× throughput; the streams drain
across all accounts at once. Run it using the host-specific long-command instructions above.

## Output

Per user, under `~/.local/state/intel/x/kols/<user>/` (lowercased):

- `tweets.jsonl`: the account's own posts, one per line — `from:<user> -filter:replies
  -filter:nativeretweets` (own posts and self-threads, not replies to others, not retweets).
- `replies.jsonl`: the account's replies to others, one per line — `from:<user> filter:replies`.
  Both hold the same fields as `fetch-x-mentions` (`id, author, author_name, author_followers,
  author_verified, text, created_at, likes, retweets, replies, quotes, views, lang, url,
  sources`; `sources` is always `[]` here), deduplicated by id, newest first.
- `posts.out.json`: progress — `{ since, until, tweets: { count, complete, tries, oldest }, replies: {...} }`.
  `oldest` is the ISO time of the oldest post held; `complete` means a genuine tail was reached.

## Existence pre-check and the timeline fallback

Before fetching a handle's timeline, the run resolves it once with `UserByScreenName`
(`X_USER_QUERY_ID`) and reads its profile: existence, `privacy.protected`, `tweet_counts.tweets`
(lifetime posts) and `created_at`. Each is settled without spending a search when it plainly has
nothing to fetch:

- **missing** (renamed / deleted / suspended): `exists: false`, streams zeroed, skipped.
- **protected** (private): its posts are never searchable; streams zeroed.
- **never-posted** (lifetime 0): streams zeroed.
- **exists**: fetched by search as usual; the profile (`id`, `lifetime`, `created`) is stored.

A handle already fully fetched is not re-resolved. Resolution is skipped if `X_USER_QUERY_ID` is unset.

**Timeline fallback (`X_TIMELINE_QUERY_ID`).** X search does not return some live accounts — brand-new
or search-deboosted ones — so `from:<user>` comes back empty even though they posted. When a live
handle whose profile shows posts (`lifetime > 0`) is empty in search, the run fetches its real
timeline directly (`UserTweetsAndReplies` by id, not search-gated), pages back to `since`, and
splits it into `tweets.jsonl` / `replies.jsonl` (retweets dropped, only the account's own posts).
A timeline that is also empty in-window confirms genuine inactivity. Skipped if the queryId is unset.

Re-extract either queryId (from x.com's authed web bundle, the `main.<hash>.js` a logged-in
`GET https://x.com/home` references) when X answers 404.

## Resume and gaps

Rerun the exact same command to resume — cheaply, because a stream **continues from where it
stopped** rather than refetching from now. A stream is `complete` only when its paging reached a
genuine tail (a short, < 20, final page). A stop on an **empty** page, a full-page boundary, or
the `--max-pages` cap leaves it incomplete (logged `(gap — rerun)`): an empty page is as often X
throttling as a true end (the reason `fetch-x-mentions` distinguishes gap days), and each account
only has ~50 pages / 15 min, so a long stream is meant to take several attempts. Each attempt
resumes at `until:<oldest+1s>` and pages further back, merging into the same file (deduplicated).
The account that stops mid-stream (a 429 wait or a pause) still saves its batch first, so no page
is refetched. An incomplete stream is retried up to `--max-tries` times, then accepted as-is; the
process exits non-zero while any stream is still worth an attempt, so **loop the command until it
exits 0** (e.g. `for i in $(seq 8); do node … && break; sleep 120; done`). Changing
`--since`/`--until` starts a fresh window (all streams reset).

## Environment Variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env`, then fill in only the values needed by the skills you use. The CLI loads that file without replacing variables already exported in the shell.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `X_BEARER_TOKEN` | X web-client bearer token | Yes | `~/.config/intel/.env` |
| `X_SEARCH_QUERY_ID` | SearchTimeline operation ID | Yes | `~/.config/intel/.env` |
| `X_TID_VERIFICATION` | Site-verification value used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_FRAME` | Animation frame data used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_ROW` | Animation row index used to sign requests | Yes | `~/.config/intel/.env` |
| `X_TID_INDICES` | Key-byte indices used to sign requests | Yes | `~/.config/intel/.env` |
| `X_USER_QUERY_ID` | UserByScreenName operation ID for the existence pre-check | No; enables pre-check | `~/.config/intel/.env` |
| `X_TIMELINE_QUERY_ID` | UserTweetsAndReplies operation ID for the timeline fallback | No; enables timeline fallback | `~/.config/intel/.env` |
| `RESIDENTIAL_PROXY_URL` | Default residential proxy | Yes | `~/.config/intel/.env` |
| `INTEL_STATE_DIR` | State root; default ~/.local/state/intel, with X archives under x/ | No | `~/.config/intel/.env` |
| `SECRETS_STATE_DIR` | Account-store directory; default ~/.local/state/secrets-manager | No | `~/.config/intel/.env` |

Account credentials (`auth_token`, `ct0`) stay in the existing Secrets Manager store, normally `~/.local/state/secrets-manager/secrets.sqlite`; do not copy them into `.env`. Capture the X web-client and signing values from x.com; refresh them when its web bundle changes.

## Failures

- `failed after N attempts` is the proxy, not a ban; rerun.
- `SearchTimeline 400` mentioning the operation/features means X redeployed; update
  `X_SEARCH_QUERY_ID` / `FEATURES` in `fetch-x-mentions`.
- `Cannot find package 'undici'` in a fresh worktree:
  `npm install --prefix "$SKILL_DIR/../fetch-x-mentions/scripts"`.

## Test

```
node --test "$SKILL_DIR/tests/fetch-x-user-posts.test.mjs"
```

Archive paths below show the default Intel state root; use the configured `INTEL_STATE_DIR` when set.

Setup shared X client: `npm install --prefix "$SKILL_DIR/../fetch-x-mentions/scripts"`.

## Shared account prerequisite

Use the existing secrets-manager CLI and store to provision or log in accounts.
It need not be installed as a Codex plugin to run its CLI. If the CLI, required
account, proxy or browser profile is missing, report the prerequisite; do not
create a second store or switch to a host browser profile.
