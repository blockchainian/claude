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
- Google by date (press from the time):
  `$G chrome google search "<query> after:2017-01-01 before:2017-03-01" -f yaml`.
- Google News: `$G chrome google news "<query>" -f yaml`.
- World news by date range, GDELT (news sites only, little on a creator the
  press has not written about): `$G gdelt "<name>" 2023-01-01 2023-12-31`
  (without dates: 2017 to today). One JSON article per line, oldest first:
  `url`, `domain`, `date` (the day GDELT collected it) and `mentions`.
  - It finds the articles in whose text GDELT recognised the name, not every
    article with the words: give a proper name as the press writes it ("The
    Kobeissi Letter", not a handle or a common word), and each other spelling
    in its own call.
  - There is no title and no cap: a year of a known name is hundreds of
    lines. Save the output to a file, count the domains, and open first the
    articles with 2 or more `mentions` (the name comes up more than in
    passing).
  - Ask for the years the subject was active, not for everything: every year
    of a name is read from BigQuery once and kept, and the month's free
    allowance is about twenty years of names.
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
- Search videos:
  `$G yt --flat-playlist --print "%(id)s %(channel)s | %(title)s" "ytsearch10:<query>"`
- Upload record (dates and plays):
  `$G yt --flat-playlist --extractor-args "youtubetab:approximate_date" --print "%(upload_date)s %(view_count)s %(title)s" "https://www.youtube.com/@<channel>/videos"`
  (`-I -20:` for the earliest twenty). The dates in this listing are
  approximate and mostly placeholders: do not build per-year counts on them.
  Read a video's own page for its exact upload date.
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

The account logged in in Chrome is a side account (joinupcomment).

- Before using it: `$G chrome twitter whoami`. If it is not joinupcomment,
  stop and report. Never use a scraper's account token.
- Posts: `$G chrome twitter tweets <handle> --limit 50 -f yaml`,
  `$G chrome twitter thread <post id> -f yaml`.
- Search: `$G fetch-x-posts "<query>" --limit 40` (newest first; `--top` by
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
  - Not `opencli twitter search`.
- One post, or an account's current follower count and join date, without
  login (`$G read` cannot read JSON, use curl here):
  `curl -s https://api.fxtwitter.com/<handle>` and
  `curl -s https://api.fxtwitter.com/<handle>/status/<id>`.
- `opencli twitter profile` returns 0 followers: do not use it.

## TikTok and Instagram

- Video list with dates and plays:
  `$G yt --flat-playlist --print "%(upload_date)s %(view_count)s %(title)s" "https://www.tiktok.com/@<account>"`
  Captions in this listing are cut at 70 characters, so a count of posts
  carrying a tag or a mention is a floor. Plays are today's cumulative totals,
  and deleted or private videos are absent.
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
  - Both keep to 30 requests a minute per exit and stop with an error when the
    archive refuses every exit: report that error as it is, do not retry
    around it or go direct.
  - A status of 429 in the output is a capture of a page that answered 429 at
    the time, not a limit on you. Pick another capture near that date.
- Before writing that a period has no archive data, list the captures for every
  URL form the profile had.
- Today's figures: `$G read "https://socialblade.com/youtube/handle/<account>"`
  (`/tiktok/user/<account>` for TikTok) gives the creation date and the
  total video count. The large number next to "followers" or "subscribers"
  on that page is likes or plays: do not misread it.

## Records, filings and books

- Lawsuits: `curl "https://www.courtlistener.com/api/rest/v4/search/?q=%22<name>%22&type=r"`.
- Securities filings: the regulator's own filing pages, fetched directly. The
  SEC's full-text search endpoint is blocked: do not use it.
- Company registries: the registry itself. Sites that resell registry data are
  not the registry; say what they are.
- Books: the `download-book` skill of this plugin, or
  `$G chrome zlibrary search "<title>"`; a PDF is read with `pdftotext`.

## The machine's settings

The gate and `wayback.mjs` read them from a `.env` file: `scripts/.env` next
to the scripts, else `~/.cache/secrets-manager/profiles/case-study/.env`.
Nothing has to be exported in the shell. A value that starts with `~/` is
under the home directory.

- `ISP_PROXY_URL`: the proxy, one URL; the ten ports after its own are the
  exits. Without it the gate reads direct, with one exit's share of the
  limits, and `wayback` goes direct.
- `FETCH_X_POSTS`: the fetch-x-posts script (X search on an account pool).
  Without it `$G fetch-x-posts` is unavailable: X search is then a gap.
- `GDELT_BQ_PROJECT`: the Google Cloud project `$G gdelt` runs its BigQuery
  queries in, with the `bq` command logged in (`gcloud auth login`). Without
  it, or once the project's free 1 TiB of queries for the month is used,
  `$G gdelt` fails: GDELT is then a gap.
