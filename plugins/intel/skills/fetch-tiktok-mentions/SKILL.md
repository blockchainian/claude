---
name: fetch-tiktok-mentions
description: Fetch a brand's TikTok videos from hashtag pages, user pages and keyword searches into docs/intel/tiktok/<slug>/ — full video metadata (videos.jsonl), each video's comments with replies (comments/<id>.jsonl) and the video files (~/.local/share/tiktok/<id>.mp4, outside the repo) — through anonymous Camoufox sessions on the ISP proxy pool, plus the logged-in TikTok account of the secrets-manager store for keyword search and full user timelines, with play-count floors, every hashtag pulled by four sessions at once, deduplicated by video id, resumable. Use when asked to fetch / 抓 / 拉 TikTok videos, comments or video files for a brand, hashtag, account or search keyword, or to add a new run to an existing TikTok archive. NOT for logging the TikTok account in (secrets-manager `login tiktok`), NOT for X (fetch-x-mentions, fetch-x-user-posts) and NOT for reading the archive.
---

# Fetch TikTok mentions

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

Hashtag pages, user profiles, keyword searches, comments and video files.
TikTok has no "everything about a brand" view, so coverage is the union of the sources you give.
Everything is fetched anonymously except keyword searches and user timelines, which go through
the logged-in account (see "The account").

Run from the repo root.

```
node \
  "$SKILL_DIR/scripts/fetch-tiktok-mentions.mjs" \
  <slug> [--hashtag <name>]... [--user <handle>]... [--keyword <words>]... \
  [--hashtag-min-plays <n>] \
  [--source-limit <n>] [--comment-limit <n>] [--sessions <n>] [--rate <n>] [--concurrency <n>] [--out <dir>] [--no-comments] [--no-download]
```

- `slug` names the output dir `docs/intel/tiktok/<slug>/`. `--out <dir>` writes to that directory
  instead (the run then need not start from the repo root).
- `--hashtag` and `--user` are repeatable; a name, `#tag` / `@handle`, or the tiktok.com url all work.
  The sources are saved, so a rerun needs only the slug; sources given later are added.
- `--keyword` is repeatable too: the words of one TikTok video search (quote several words). The
  search covers captions, on-screen text, hashtags and speech, so it also finds videos that mention
  the brand without its hashtag. It needs the account.
- `--hashtag-min-plays` (default 10000) drops a hashtag page's videos below that many plays. A
  user's videos and a keyword's results have no floor.
- Only English videos are kept, from every source: TikTok's `textLanguage` must be `en` or `un`
  (a caption it could not tell, in practice hashtags alone).
- `--source-limit` caps the videos one pull of a source yields (default 1000).
- `--comment-limit` caps the comments kept per video, top-level and replies together (default 1000).
  Every top-level comment is taken first; the room left goes to replies, the most liked and
  replied threads first, a page (20) per thread in turn.
- `--sessions` is how many browser sessions work at once, one per ISP slot (default: every slot).
- `--rate` is how many requests one session starts per second (default 20); `--concurrency` how many
  it may have in flight (default 12). File downloads run 4 per session: they share the slot's
  bandwidth, more only time out.
- `--no-comments` / `--no-download` skip a phase.

Invented example (Demo Fun):

```
node \
  "$SKILL_DIR/scripts/fetch-tiktok-mentions.mjs" \
  demofun --hashtag demofun --hashtag demodotfun --user demo.fun --keyword "demo fun"
```

Every run does three things: collects every source (a hashtag through four sessions at once, see below; new videos are
added, held ones get fresh stats), fetches comments for the videos whose comments are not
complete, and downloads the videos without a file. Use the host-specific long-command instructions above; read the
log. **Rerun the same command until it exits 0.**

The default rate is 20 requests/s per anonymous session. Tune `--rate` and `--concurrency`
for the target sources and proxy pool. A refused session (`Access Denied`) is replaced;
empty responses trigger a one-minute cool-down before reusing the slot.

What each source gives:

| Source | Depth |
|---|---|
| hashtag page | A sample in TikTok's own order, mixing popular and fresh videos. Several sessions pull once each and merge the samples; overlap and coverage depend on the source and IP region. |
| user profile | With an account: pages through the timeline. Without one: may repeat the first page; the timeline is then logged `(incomplete)`. |
| keyword search | TikTok's own relevance order. Needs the account; results may overlap hashtag and user sources. |

## The account

A keyword search is answered only to a logged-in viewer, and a user's timeline past its first page
too. Both go through the TikTok account the secrets-manager skill logged in (`login tiktok`): the
first `active` row of its `tiktok` table. It is opened as the same browser profile it logged in
with (`<state>/profiles/<username>`, the same device to TikTok), on the ISP slot it logged in
from, one request at a time at 2 requests/s. That slot also carries an anonymous session; two
sessions share the slot.

- No active account: keywords are not collected (the run says so and exits non-zero), user
  timelines stay one page, everything else runs.
- The profile is no longer signed in: the account is marked `expired` in the store; run
  `login tiktok` again.
- `search refused: <code>`: TikTok answered the search with an error instead of results. The
  account is marked `expired`; run `login tiktok`, which checks the profile against tiktok.com
  and signs in again only if it is really logged out.

## Output

Under `docs/intel/tiktok/<slug>/`, or the `--out` directory:

- `videos.jsonl`: one video per line, TikTok's full item (`id, desc, createTime, author, stats,
  challenges, music, textExtra, video, ...`) plus `sources`, the pages that surfaced it
  (`#tag`, `@user`, `"keyword"`). Deduplicated by id, newest first.
- `comments/<videoId>.jsonl`: one comment per line, TikTok's full comment (`cid, text, create_time,
  digg_count, user, reply_id, reply_comment_total, ...`). Top-level comments are ordered most popular
  first (likes + replies). Each top-level comment (`reply_id` `"0"`)
  is followed by its replies (`reply_id` = the parent's `cid`).
- `videos.out.json`: progress —
  `{ sources: { hashtags, users, keywords }, commentLimit, runs: [{ at, fetched, kept, new, sources: { "<label>": { fetched, kept, new, complete, pulls } } }], videos: { "<id>": { comments: { count, complete }, downloaded } } }`.
  Per run and source: `fetched` videos TikTok returned, `kept` those at or above the play floor and in English,
  `new` those kept and not held before, `pulls` how often the source was paged.

Video files are not in the repo: `~/.local/share/tiktok/<videoId>.mp4`, one copy shared by every
slug, so they survive a deleted worktree. `TIKTOK_VIDEOS_DIR` overrides the directory. Photo posts
have no video file. A video TikTok no longer has is logged `gone` and asked for again next run
(the refusal is often temporary); it does not make the run exit non-zero.

TikTok hides part of a busy video's comments from an anonymous viewer ("folded" comments): a video
may show more comments than its API returns. In a synthetic example, a displayed count of
2,000 could yield only 120 visible top-level comments. What is saved is what the web page shows.
Link a reply to its parent by `reply_id`, not by position.

A video's comments are `complete` once paged to the end or to the limit; they are not fetched
again, so comments posted later are not picked up. A video TikTok reports 0 comments on is
recorded complete without a request.

## How it works

TikTok's web API answers an unsigned request with an empty 200. Each session opens tiktok.com in
Camoufox, copies the query params of the first API request the page itself sends, and calls
`fetch()` inside the page, where TikTok's own script signs it. One session is one browser on one
ISP slot (a fixed IP), shared by several lanes. A non-JSON answer (`Access Denied`) is held against
the session: it is closed and a fresh one on the same slot takes the work over at once. An empty
answer is held against the IP: the slot's next session opens after a one-minute cool-down.

Video files are fetched inside the session too: `item/detail/` gives a fresh play address, signed
for the session's IP, and the page's `fetch()` pulls the bytes, four files at a time per session.

## Config

- `$SKILL_DIR/scripts/.env`: `ISP_PROXY_URL` (the pool's base url) and `ISP_PROXY_COUNT`
  (slot n is the base port + n, one fixed IP each).
- `SECRETS_MANAGER_STATE_PATH` (default `~/.config/secrets-manager`): where the account's store
  and browser profile are.
- First use in a checkout: `npm install --prefix "$SKILL_DIR/scripts"`. The Camoufox
  browser is the one secrets-manager installs (`npx camoufox-js fetch`).

## Failures

- `blocked, replacing the session` now and then is normal. Lanes ending with
  `stopped after 3 failures in a row` mean their IP stays refused: the other slots finish what
  they can; wait and rerun, or lower `--concurrency`.
- `the page sent no API request`: the page was slow or got a challenge; the lane retries.
- Every request blocked from the first one: TikTok changed its page. Open tiktok.com/explore,
  check that its `/api/` requests still carry `device_id` and are signed by the page's `fetch`.
- `download failed`: the play address answered non-200; rerun.

## Test

```
node --test "$SKILL_DIR/tests/fetch-tiktok-mentions.test.mjs"
```

Archive paths are relative to the working directory, run from the repo root that owns the archive.

Log the account in with the `secrets` plugin’s `secrets-manager login tiktok`.

`$SKILL_DIR/scripts/.env` loads automatically without replacing existing environment values.
Keys: `X_BEARER`, `X_SEARCH_QUERY_ID`, `X_USER_QUERY_ID`, `X_USER_TWEETS_QID`,
`X_TID_VERIFICATION`, `X_TID_FRAME`, `X_TID_ROW`, `X_TID_INDICES`,
`RESIDENTIAL_PROXY_URL`, `X_PROXY_URLS`, `ISP_PROXY_URL`, `ISP_PROXY_COUNT`.
Only keys needed by this script are required. Missing required keys report this config path.

## Shared account prerequisite

Use the existing secrets-manager CLI and store to provision or log in accounts.
It need not be installed as a Codex plugin to run its CLI. If the CLI, required
account, proxy or browser profile is missing, report the prerequisite; do not
create a second store or switch to a host browser profile.
