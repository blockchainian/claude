---
name: fetch-x-user-posts
description: Fetch an X/Twitter account's own posts and replies over a date range into docs/intel/x/kols/<user>/tweets.jsonl and replies.jsonl — the authenticated SearchTimeline `from:<user>` paged chronologically, sharded across X accounts through the residential proxy, resumable, for a batch of usernames at once. Use when asked to fetch / 抓 / 拉 one or many accounts' own timelines (e.g. a KOL roster's past-year posts). NOT for posts that mention an app (use fetch-x-mentions), NOT for a few of an account's posts without saving them (use fetch-x-posts) and NOT for reading the archive (analyze-x-user).
---

# Fetch X users

Fetch what accounts post themselves — their own tweets and their replies — as opposed to
`fetch-x-mentions`, which fetches what everyone says about an app. Give it a batch of screen
names; it writes one folder per user. The auth core (x-client-transaction-id, the account list,
the SearchTimeline request, page parsing, retry/quota handling, cross-account draining) is
imported from `fetch-x-mentions`, so its `~/.config/intel/.env` is the single source of secrets.

Run from the repo root.

```
node \
  "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-user-posts/scripts/fetch-x-user-posts.mjs" \
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
  "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-user-posts/scripts/fetch-x-user-posts.mjs" \
  --file docs/intel/kols/2026-09-24.json --since 2025-09-24 --until 2026-09-24
```

Every X account has its own rate bucket, so N accounts ≈ N× throughput; the streams drain
across all accounts at once. Launch it in the background and reread the log, don't tail it.

## Output

Per user, under `docs/intel/x/kols/<user>/` (lowercased):

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

**Timeline fallback (`X_USER_TWEETS_QID`).** X search does not return some live accounts — brand-new
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

## Config

Uses `~/.config/intel/.env` and the same account store
(`~/.config/secrets-manager/secrets.sqlite`, active X rows) and `X_SEARCH_QUERY_ID / X_BEARER / X_TID_*`
as `fetch-x-mentions`. Additionally: `X_USER_QUERY_ID` (UserByScreenName) and `X_USER_TWEETS_QID`
(UserTweetsAndReplies) for the pre-check and the timeline fallback.

## Failures

- `failed after N attempts` is the proxy, not a ban; rerun.
- `SearchTimeline 400` mentioning the operation/features means X redeployed; update
  `X_SEARCH_QUERY_ID` / `FEATURES` in `fetch-x-mentions`.
- `Cannot find package 'undici'` in a fresh worktree:
  `npm install --prefix "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts"`.

## Test

```
node --test "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-user-posts/tests/fetch-x-user-posts.test.mjs"
```

Archive paths are relative to the working directory, run from the repo root that owns the archive.

`~/.config/intel/.env` loads automatically without replacing existing environment values.
Keys: `X_BEARER`, `X_SEARCH_QUERY_ID`, `X_USER_QUERY_ID`, `X_USER_TWEETS_QID`,
`X_TID_VERIFICATION`, `X_TID_FRAME`, `X_TID_ROW`, `X_TID_INDICES`,
`RESIDENTIAL_PROXY_URL`, `X_PROXY_URLS`, `ISP_PROXY_URL`, `ISP_PROXY_COUNT`.
Only keys needed by this script are required. Missing required keys report this config path.

Setup shared X client: `npm install --prefix "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts"`.
