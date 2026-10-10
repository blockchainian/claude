---
name: fetch-tiktok-mentions
description: Fetch a brand's TikTok videos from hashtag pages, user pages and keyword searches into a per-brand archive under the intel state root — full video metadata, each video's comments with replies, and the video files — through anonymous Camoufox sessions on the ISP proxy pool, plus the secrets-manager store's logged-in TikTok account for keyword search and full user timelines; play-count floors, deduplicated by video id, resumable. Use when asked to fetch / 抓 / 拉 TikTok videos, comments or video files for a brand, hashtag, account or search keyword, or to add a new run to an existing TikTok archive. NOT for logging the TikTok account in (secrets-manager `login tiktok`), NOT for X (fetch-x-mentions, fetch-x-user-posts) and NOT for reading the archive.
---

# Fetch TikTok mentions

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The script
imports from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin's `.env.example` to `~/.config/intel/.env` and fill in only the values the
skills you use need. The script loads that file without replacing variables already exported in
the shell. Account credentials, login sessions and browser profiles stay in the Secrets Manager
store; never copy them into `.env`.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ISP_PROXY_URL` | ISP proxy pool base URL | Yes | `~/.config/intel/.env` |
| `ISP_PROXY_COUNT` | Number of pool slots; default 1 | No | `~/.config/intel/.env` |
| `SECRETS_STATE_DIR` | Account-store and browser-profile directory; default ~/.local/state/secrets-manager | No | `~/.config/intel/.env` |
| `INTEL_STATE_DIR` | State root; default ~/.local/state/intel, with TikTok data under `tiktok/` | No | `~/.config/intel/.env` |

## Setup

Install the dependencies once per checkout, and use the Camoufox browser that secrets-manager
installs (`npx camoufox-js fetch`):

```sh
npm install --prefix "$SKILL_DIR/scripts"
```

Provision and log in accounts only with the existing secrets-manager CLI and store; its CLI runs
without being installed as a Codex plugin. If the CLI, the account, the proxy or the browser
profile is missing, report the missing prerequisite. Never create a second store or fall back to
a host browser profile.

## Run

TikTok has no view of everything about a brand, so coverage is the union of the hashtags, users
and keywords you give. Everything is fetched anonymously except keyword searches and user
timelines past their first page, which go through the logged-in account. The script runs from any
working directory. An invented example for a brand called Demo Fun:

```sh
node "$SKILL_DIR/scripts/fetch-tiktok-mentions.mjs" \
  demofun --hashtag demofun --hashtag demodotfun --user demo.fun --keyword "demo fun"
```

Each run collects every source, adding new videos and refreshing the stats of held ones, then
fetches comments for videos whose comments are not complete, then downloads videos that have no
file yet. Run it in the background, read the log, and **rerun the same command until it exits 0**.
The sources are saved with the archive, so a rerun needs only the slug, and sources given on a
later run are added to the saved ones.

Only English videos are kept, from every source: TikTok's `textLanguage` must be `en` or `un`, a
caption it could not tell, which in practice means hashtags alone. What each source yields:

| Source | Depth |
|---|---|
| hashtag page | A sample in TikTok's own order, mixing popular and fresh videos. Up to four sessions pull it at once and the samples are merged; overlap and coverage depend on the source and the IP region. |
| user profile | With the account, the whole timeline is paged. Without it, the profile may repeat its first page, and the log marks the timeline `(incomplete)`. |
| keyword search | TikTok's relevance order over captions, on-screen text, hashtags and speech, so it also finds videos that mention the brand without its hashtag. Needs the account; results may overlap the hashtag and user sources. |

## The account

TikTok answers a keyword search, and a user's timeline past its first page, only to a logged-in
viewer. Both go through the TikTok account that secrets-manager logged in with `login tiktok`: the
first `active` row of its `tiktok` table, opened in the browser profile it logged in with (the
same device to TikTok), on the ISP slot it logged in from, one request at a time at 2 requests per
second. That slot also carries an anonymous session.

- With no active account, keywords are not collected and the run says so and exits non-zero; user
  timelines stay at one page and everything else runs.
- When the profile is no longer signed in, the script marks the account `expired` in the store;
  run `login tiktok` again.
- `search refused: <code>` means TikTok answered the search with an error instead of results. The
  account is marked `expired`; run `login tiktok`, which checks the profile against tiktok.com and
  signs in again only if it is really logged out.

## Output

The archive sits in `tiktok/<slug>/` under the state root, or in the `--out` directory:

- `videos.jsonl` holds one video per line, TikTok's full item (`id, desc, createTime, author,
  stats, challenges, music, textExtra, video, ...`) plus `sources`, the pages that surfaced it
  (`#tag`, `@user`, `"keyword"`), deduplicated by id and newest first.
- `comments/<videoId>.jsonl` holds one comment per line, TikTok's full comment (`cid, text,
  create_time, digg_count, user, reply_id, reply_comment_total, ...`). Top-level comments
  (`reply_id` `"0"`) come most popular first by likes plus replies, each followed by its replies
  (`reply_id` is the parent's `cid`). Link a reply to its parent by `reply_id`, not by position.
- `videos.out.json` records progress:
  `{ sources: { hashtags, users, keywords }, commentLimit, runs: [{ at, fetched, kept, new, sources: { "<label>": { fetched, kept, new, complete, pulls } } }], videos: { "<id>": { comments: { count, complete }, downloaded } } }`.
  Per run and source, `fetched` counts the videos TikTok returned, `kept` those in English and at
  or above the play floor, `new` those kept and not held before, and `pulls` how often the source
  was paged.

Video files live outside the archive, as `tiktok/<videoId>.mp4` under the state root: one copy
shared by every slug, so they survive a deleted worktree. Photo posts have no video file. A video
TikTok no longer has is logged `gone` and asked for again on the next run, since the refusal is
often temporary; it does not make the run exit non-zero.

A video's comments are `complete` once paged to the end or to the limit, and are never fetched
again, so comments posted later are not picked up. A video TikTok reports 0 comments on is
recorded complete without a request. TikTok folds part of a busy video's comments away from an
anonymous viewer, so a video may show more comments than its API returns; for example, a
displayed count of 2,000 could yield only 120 top-level comments. What is saved is what the web
page shows.

## Options

```
node "$SKILL_DIR/scripts/fetch-tiktok-mentions.mjs" \
  <slug> [--hashtag <name>]... [--user <handle>]... [--keyword <words>]... \
  [--hashtag-min-plays <n>] \
  [--source-limit <n>] [--comment-limit <n>] [--sessions <n>] [--rate <n>] [--concurrency <n>] [--out <dir>] [--no-comments] [--no-download]
```

- `<slug>` names the archive directory; `--out <dir>` writes to that directory instead.
- `--hashtag` and `--user` are repeatable and take a name, `#tag` or `@handle`, or the tiktok.com
  URL.
- `--keyword` is repeatable and takes the words of one TikTok video search; quote several words.
- `--hashtag-min-plays` (default 10000) drops a hashtag page's videos below that many plays. A
  user's videos and a keyword's results have no floor.
- `--source-limit` (default 1000) caps the videos one pull of a source yields.
- `--comment-limit` (default 1000) caps the comments kept per video, top-level and replies
  together. Every top-level comment is taken first; the room left goes to replies, the most liked
  and replied threads first, one page of 20 per thread in turn.
- `--sessions` (default every slot) sets how many browser sessions work at once, one per ISP slot.
- `--rate` (default 20) sets how many requests one anonymous session starts per second, and
  `--concurrency` (default 12) how many it may have in flight; tune both to the target sources and
  the proxy pool. File downloads run four per session, because they share the slot's bandwidth and
  more only time out.
- `--no-comments` and `--no-download` skip those phases.

## Failures

Each session opens tiktok.com in Camoufox and calls TikTok's API through the page's own `fetch()`,
which signs the request; an unsigned request gets an empty 200.

- `blocked, replacing the session` now and then is normal: a session answered with `Access
  Denied` is replaced at once, and an empty answer makes its slot cool down for a minute first.
  Workstreams ending with `stopped after 3 failures in a row` mean their IP stays refused; the
  other slots finish what they can, so wait and rerun, or lower `--concurrency`.
- `the page sent no API request` means the page was slow or got a challenge; the workstream
  retries.
- When every request is blocked from the first one, TikTok has changed its page. Open
  tiktok.com/explore and check that its `/api/` requests still carry `device_id` and are signed by
  the page's `fetch`.
- `download failed` means the play address answered non-200; rerun.

## Tests

```sh
node --test "$SKILL_DIR/tests/fetch-tiktok-mentions.test.mjs"
```
