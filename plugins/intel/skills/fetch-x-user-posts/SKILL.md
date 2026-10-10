---
name: fetch-x-user-posts
description: Fetch X/Twitter accounts' own posts and replies over a date range into one tweets.jsonl and replies.jsonl per account — the authenticated SearchTimeline `from:<user>` paged chronologically, sharded across X accounts through the residential proxy, resumable, for a batch of usernames at once. Use when asked to fetch / 抓 / 拉 one or many accounts' own timelines (e.g. a KOL roster's past-year posts). NOT for posts that mention an app (use fetch-x-mentions), NOT for a few of an account's posts without saving them (use fetch-x-posts) and NOT for reading the archive (analyze-x-user).
---

# Fetch X user posts

This skill fetches what accounts post themselves, their own tweets and their replies, for a batch
of screen names, and writes one folder per user. The X client it uses (request signing, the
account list, SearchTimeline requests, retries and quota handling) comes from the sibling
`fetch-x-mentions` skill.

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from `fetch-x-mentions`, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin's `.env.example` to `~/.config/intel/.env` and fill in the values the skills
you use need. Variables already exported in the shell take precedence over the file.

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
| `INTEL_STATE_DIR` | State root; default `~/.local/state/intel`, with X archives under `x/` | No | `~/.config/intel/.env` |
| `SECRETS_STATE_DIR` | Secrets Manager account-store directory; default `~/.local/state/secrets-manager` | No | `~/.config/intel/.env` |

Capture the web-client and signing values from x.com, and refresh them when its web bundle
changes. Account credentials (`auth_token`, `ct0`) stay in the Secrets Manager store (`secrets.sqlite`) under
`SECRETS_STATE_DIR`; never copy them into `.env`.

## Setup

Install the shared X client's dependencies once:

```sh
npm install --prefix "$SKILL_DIR/../fetch-x-mentions/scripts"
```

Provision and log in X accounts with the existing secrets-manager CLI and store; the CLI runs
without being installed as a Codex plugin. If the CLI, a required account, the proxy or a browser
profile is missing, report the prerequisite. Never create a second store or switch to a host
browser profile.

## Run

Run from any working directory, in the background, because a batch takes a long time:

```sh
node "$SKILL_DIR/scripts/fetch-x-user-posts.mjs" \
  <user> [<user> ...] [--file <path>] [--since YYYY-MM-DD] [--until YYYY-MM-DD] \
  [--max-pages <n>] [--max-tries <n>]
```

- `<user>` is a bare screen name. A leading `@`, an `x.com/…` URL or a trailing path is stripped,
  and invalid names are dropped. Pass as many as you like.
- `--file` adds a newline-separated list, or a JSON object whose values are arrays of usernames.
  Arguments and files are merged, validated and deduplicated.
- `--since` and `--until` are UTC days, and `--until` is exclusive. `--until` defaults to today and
  `--since` to one year before `--until`.
- `--max-pages` caps each attempt at `n` pages of 20 posts (default 500, which is 10 000 posts).
  A longer stream continues in the next attempt, so this guards one attempt rather than capping
  the total.
- `--max-tries` is how many attempts an incomplete stream gets before it is accepted as it is
  (default 3; use 5 for a big overnight batch). Each X account's SearchTimeline quota is about 50
  pages per 15 minutes, so a very active account's year takes several attempts on rotating
  accounts.

Each X account has its own rate bucket, and the streams drain across all accounts at once, so N
accounts give roughly N times the throughput. For example, a whole KOL roster's past year:

```sh
node "$SKILL_DIR/scripts/fetch-x-user-posts.mjs" \
  --file docs/intel/kols/2026-09-24.json --since 2025-09-24 --until 2026-09-24
```

## Output

Each user gets a lowercased folder `x/kols/<user>/` under the state root holding three files:

- `tweets.jsonl` holds the account's own posts and self-threads, without replies to others or
  retweets (`from:<user> -filter:replies -filter:nativeretweets`).
- `replies.jsonl` holds its replies to others (`from:<user> filter:replies`).
- `posts.out.json` records progress as `{ since, until, tweets: { count, complete, tries, oldest },
  replies: {...} }`, where `oldest` is the ISO time of the oldest post held and `complete` means
  the stream reached its genuine end.

Both JSONL files carry one post per line with the same fields as `fetch-x-mentions` (`id, author,
author_name, author_followers, author_verified, text, created_at, likes, retweets, replies, quotes,
views, lang, url, sources`, with `sources` always `[]` here), deduplicated by id, newest first.

## Existence pre-check and timeline fallback

When `X_USER_QUERY_ID` is set, the run first resolves each handle once with `UserByScreenName` and
reads its existence, `privacy.protected`, lifetime post count (`tweet_counts.tweets`) and
`created_at`. A handle with plainly nothing to fetch is settled without spending a search:

- **missing** (renamed, deleted or suspended): `exists: false`, both streams zeroed, skipped.
- **protected** (private): its posts are never searchable, so both streams are zeroed.
- **never-posted** (lifetime 0): both streams zeroed.
- **exists**: fetched by search as usual, with its profile (`id`, `lifetime`, `created`) stored.

A handle already fully fetched is not resolved again.

X search does not return some live accounts, typically brand-new or search-deboosted ones, so
`from:<user>` comes back empty although they posted. When `X_TIMELINE_QUERY_ID` is set and a live
handle whose profile shows posts is empty in both searches, the run fetches its real timeline with
`UserTweetsAndReplies` by id, which search does not gate, pages back to `--since`, drops retweets
and other people's posts, and splits the rest into `tweets.jsonl` and `replies.jsonl`. A timeline
that is also empty inside the window confirms the account was genuinely inactive.

When X answers 404 to either lookup, re-extract its query ID from x.com's authenticated web bundle,
the `main.<hash>.js` that a logged-in `GET https://x.com/home` references.

## Resume and gaps

To resume, rerun the exact same command. Each stream continues from where it stopped instead of
refetching from now: an attempt pages back from `until:<oldest+1s>` and merges into the same file,
deduplicated. An account that stops mid-stream, on a 429 wait or a pause, still saves its batch first, so no page is
fetched twice.

A stream is complete when its paging ends on a short final page (fewer than 20 posts), or when
results run out while the user holds posts in either stream. It stays incomplete, logged as `gap`,
when it hits the `--max-pages` cap or a bad response, or when results run out for a user with no
posts at all, since an empty page is as often X throttling as a true end. An incomplete stream is
retried up to `--max-tries` times and then accepted as it is.

The process exits non-zero while any stream is still worth another attempt, so loop the command
until it exits 0, for example `for i in $(seq 8); do node … && break; sleep 120; done`. Changing
`--since` or `--until` starts a fresh window and resets every stream.

## Failures

- `failed after N attempts` comes from the proxy, not a ban; rerun.
- `SearchTimeline 400` mentioning the operation or features means X redeployed; update
  `X_SEARCH_QUERY_ID`, or `FEATURES` in `fetch-x-mentions`.
- `Cannot find package 'undici'`, as in a fresh worktree, means the shared client's dependencies
  are missing; run the install in Setup.

## Tests

```sh
node --test "$SKILL_DIR/tests/fetch-x-user-posts.test.mjs"
```
