# Tools

Every third-party service is called through the gate, `<this skill>/scripts/gate.mjs`
(the skill folder is the one that holds this file's folder): many agents run at
once and the limits are per machine, so the gate queues, paces, changes exit
and retries for all of them. Never go around it with `curl r.jina.ai`,
`mcporter call exa...`, `opencli ...`, `yt-dlp ...` or `wayback.mjs` directly.
It may wait in a queue: give its commands a Bash timeout of 300000 or more.
Use the commands as written; do not spend the run testing tools. Everything is
read-only: never post, comment, like or follow.

`G` below stands for `<this skill>/scripts/gate.mjs`. `gate.mjs stats` prints
calls and failures per command.

## Search

- Exa, with the engines behind it when Exa is out of credits:
  `$G search "<query>" 8` (the second argument is the number of results).
  Not for news: news is the two news commands below.
- Google News by date range: `$G gnews "<name>" 2023-01-01 2023-12-31`
  (without dates: 2017 to today), the name without quotes. One JSON article
  per line, oldest first: `url` (the article's own address), `domain`,
  `date`, `title`.
- World news by date range, GDELT (news sites only, little on a creator the
  press has not written about):
  `$G gdelt "<name>" "<another name>" 2023-01-01 2023-12-31` (up to 100
  names; without dates: 2017 to today). One JSON article per line, each name
  oldest first: `name`, `url`, `domain`, `date` (the day GDELT collected it),
  `mentions` (how many times the name was found in the article).
  - It finds the articles in whose text GDELT recognised the name, not every
    article with the words, and there is no title.
- Both news commands have no cap: a year of a known name is hundreds of
  lines. Save the output to a file and count the domains before opening any.
  What they fetched is kept under `~/.local/state/intel/case-study/news/`, a folder per
  name, so asking again for days already held costs nothing.
- Reddit: `$G chrome reddit search "<query>" -f yaml`, `$G chrome reddit read <post id>`.

## Reading a page

In this order:

1. `$G read "<URL>"` — the Jina reader over the proxy exits, then Exa, then
   Chrome; the output is the page text.
2. `$G chrome web read --url "<URL>" --stdout true --download-images false --window background`
   — the user's own Chrome, with its logins.

- Read in full by 1: Time, The Verge, Rolling Stone, Business Insider,
  Bloomberg.
- Forbes articles, Hollywood Reporter, the New York Times (subscribed, logged
  in in Chrome): only 2 reads them in full; go straight to 2.
- Not readable in full: the Wall Street Journal, The Information. Their
  articles read only to the teaser: a gap, not a source.
- A PDF: download it, then `pdftotext <file.pdf> -`.

## YouTube

The gate adds the Chrome login itself: never add `--cookies-from-browser`.

- Subtitles of one video:
  `$G yt --write-sub --write-auto-sub --sub-lang en --skip-download -o "<work>/raw/<batch>/%(id)s" "<URL>"`
- Search videos: `$G ytsearch "<query>" "<name>" "<another name>"`. One JSON
  video per line: `id`, `url`, `channel`, `title`. Only the videos whose
  title, channel or description holds one of the names are listed: YouTube
  fills a search up with videos that have nothing to do with it.
- Upload record, every upload of a channel in one command:
  `$G ytuploads "https://www.youtube.com/@<channel>" > <work>/raw/uploads/uploads.jsonl`
  One JSON upload per line, oldest first: `id`, `kind` (`videos`, `shorts`
  or `streams`), `date` (the exact upload day, from the video's own page),
  `timestamp`, `views` (today's total), `duration` (seconds), `title`, `url`.
  - It reads every video's page, many at once, through the proxy's exits and
    without the login: about 1,000 uploads in 3 minutes. Run it once and work from the file; do not
    read video pages one by one for their dates.
  - The pages it could not read (private or removed videos) are named on
    stderr: a gap. "YouTube refused every route" means no page was read:
    write it into the gaps and do not fall back to reading pages one by one.
- Not `opencli youtube transcript`: it is broken.
- The login is shared by the whole machine: download subtitles for the
  interviews you will read, never loop over a channel. "Sign in to confirm"
  in the output means stop using YouTube and write it into the gaps.

## Podcasts with no video

- Find the show: `$G chrome apple-podcasts search "<show>"`,
  `$G chrome apple-podcasts episodes <id>`.
- Transcribe: the `transcribe` skill of this plugin
  (`${CLAUDE_PLUGIN_ROOT}/skills/transcribe`, local whisper). When the same
  episode is on YouTube, subtitles are faster.

## X

Every post on X — a search, an account's own posts, a thread — comes from
`$G fetch-x-posts`, and from nothing else.

- `$G fetch-x-posts "<query>" --limit 40` (newest first; `--top` by
  engagement). One JSON post per line: id, url, created_at (UTC), user, text
  (in full), likes, retweets, replies, views, quoted, in_reply_to. It runs on
  an account pool, not on the side account: search freely.
  - The query goes to X as written; operators work: `from:<handle>`,
    `min_faves:1000`, `filter:replies`, `-filter:replies`, `"exact phrase"`.
  - Dates carry a UTC time or the window drifts:
    `since:2022-04-01_00:00:00_UTC until:2022-05-01_00:00:00_UTC`.
  - An account's past: its earliest posts (`from:<handle> until:2009-06-01_00:00:00_UTC`),
    a month's hits (`from:<handle> since:... until:... min_faves:5000`),
    a month's count (`--limit 400`, then count lines; 20 per request, 400 in
    about 40 seconds).
  - A non-zero exit is a failure and the last stderr line says why: write it
    into the gaps as it is. Exit 0 with no output is a true empty result.
  - An account's own posts: `from:<handle>`; a thread: `conversation_id:<post id>`.
- An account's current follower count and join date (not posts):
  `curl -s https://api.fxtwitter.com/<handle>`.

## TikTok and Instagram

- An account's videos, every one with its date, full caption and counts
  (plays, likes, comments, shares), through the fetch-tiktok-mentions skill
  next to this one (it paces itself; it is not called through the gate):
  `node <this skill>/../fetch-tiktok-mentions/scripts/fetch-tiktok-mentions.mjs <account> --user <account> --out <work>/raw/tiktok --no-comments --no-download`
  It writes `<work>/raw/tiktok/videos.jsonl`, one video per line, newest
  first (`createTime`, `desc`, `stats`), and continues when run again: rerun
  it until it exits 0.
  - Only videos with an English caption are kept. Deleted or private videos
    are absent, and plays are today's cumulative totals.
  - `(incomplete)` after the account in its last lines means the timeline
    was not read to the end: it stopped at 1,000 videos (give
    `--source-limit <n>` for more), or the TikTok account it reads through
    is logged out. What is still missing is a gap, with the date of the
    oldest video it reached.
  - "Cannot find package": run
    `npm install --prefix <this skill>/../fetch-tiktok-mentions/scripts` once.
- `opencli tiktok` and `opencli instagram` are not logged in: Instagram data
  comes from the press and the archive only.

## Follower history from the Wayback Machine

- List captures: `$G chrome archive snapshots "<url>" --limit 400 -f json`.
- Old YouTube channel pages (`/user/<name>`, `/channel/<id>`) carry
  "N subscribers" (`youtube.com/user/MrBeast6000` on 2016-03-07: 19,936; 400
  captures from 2016 to 2019). Archived Social Blade pages carry followers,
  uploads and a daily table (`socialblade.com/tiktok/user/kallmekris` on
  2021-03-17: 25.4M followers, 1,011 videos): this is how TikTok's follower
  history is read. The "last 30 days" box on those pages can be wrong; use
  the daily table. Archived `twitter.com/<handle>` pages before 2022-01
  carry the follower count (visible profile and JSON-LD
  `"name": "Follows", "userInteractionCount": N`); later ones do not. Some
  captures are empty shells; try neighbouring captures and `mobile.twitter.com`.
- Read captures through the gate, never one `curl` at a time; one agent at a
  time uses the archive, the others queue:
  - The whole curve, for every address the profile has had:
    `$G wayback curve <work>/raw/archive <address>...`
    It lists the monthly captures, fetches them in one batch, saves every
    page, and prints one JSON line per capture: date, the count it read
    (`value`), the page's own wording when the count is rounded or in another
    language (`text`), the capture URL and the saved file.
  - Chosen captures in one batch: put the capture URLs
    (`https://web.archive.org/web/<timestamp>id_/<url>`) in a file, then
    `$G wayback fetch <work>/raw/archive --from <file>`.
  - Captures go through the residential proxy at 100 requests a minute (240
    captures take 3 minutes); the capture lists go through the ISP proxy at
    100 a minute. A request that failed is asked again: a capture at once,
    through another address; a list after a pause of the whole batch. What
    keeps failing is named in the error, and
    `curve` still prints the rows it got first: report what is missing as it
    is, do not retry around it or go direct. Give `curve` every address in
    one call.
  - A status of 429 in the output is a capture of a page that answered 429 at
    the time, not a limit on you. Pick another capture near that date.
- Before writing that a period has no archive data, list the captures for every
  URL form the profile had.
- Today's figures: `$G read "https://socialblade.com/youtube/handle/<account>"`
  (`/tiktok/user/<account>` for TikTok) gives the creation date and the
  total video count. The large number next to "followers" or "subscribers"
  on that page is likes or plays: do not misread it.

## Records, filings and books

- Securities filings: the regulator's own filing pages, fetched directly. The
  SEC's full-text search endpoint is blocked: do not use it.
- Company registries: the registry itself. Sites that resell registry data are
  not the registry; say what they are.
- Books: the `download-book` skill of this plugin; a PDF is read with
  `pdftotext`.

## The machine's settings

The gate and `wayback.mjs` read them from Intel’s `~/.config/intel/.env`.
Nothing has to be exported in the shell. A value that starts with `~/` is
under the home directory.

- `ISP_PROXY_URL`: the proxy, one URL; the ten ports after its own are the
  exits. Without it the gate reads direct, with one exit's share of the
  limits.
- `RESIDENTIAL_PROXY_URL`: the rotating residential proxy `wayback` reads the
  archive's captures through, one URL; without it `wayback` stops with an
  error. The capture lists go through `ISP_PROXY_URL`. `gnews` asks Google News
  through an `ISP_PROXY_URL` exit and again through this proxy when Google
  refuses the exit.
The X-post fetching command locates its sibling script inside the installed Intel plugin; no script-path variable is needed.
- `BIGQUERY_PROJECT_ID`: the Google Cloud project `$G gdelt` runs its BigQuery
  queries in, with the `bq` command logged in (`gcloud auth login`). Without
  it, or once the project's free 1 TiB of queries for the month is used,
  `$G gdelt` fails: GDELT is then a gap.
