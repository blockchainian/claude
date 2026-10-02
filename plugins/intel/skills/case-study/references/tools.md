# Tools

If the invocation gave a tool list file, read it and prefer its commands: they
were tested on this machine and cover its logins and paid readers. The commands
below need no login and are the fallback. Use them as written; do not spend
the run testing tools. Everything is read-only: never post, comment, like or
follow.

## Reading a page

- `curl -s "https://r.jina.ai/<URL>"` returns the page as text.
- A PDF: download it, then `pdftotext <file.pdf> -`.
- A paywalled page read only to its teaser is a gap, not a source.

## Press from the time

- A search engine with a date range (`after:YYYY-MM-DD before:YYYY-MM-DD`).
- GDELT, by date range:
  `curl "https://api.gdeltproject.org/api/v2/doc/doc?query=%22<name>%22&mode=artlist&maxrecords=50&format=json&startdatetime=YYYYMMDD000000&enddatetime=YYYYMMDD235959"`

## YouTube

- Subtitles of one video:
  `yt-dlp --write-sub --write-auto-sub --sub-lang en --skip-download -o "<work>/raw/%(id)s" "<URL>"`
- Upload record:
  `yt-dlp --flat-playlist --extractor-args "youtubetab:approximate_date" --print "%(upload_date)s %(view_count)s %(title)s" "https://www.youtube.com/@<channel>/videos"`
  (`-I -20:` for the earliest twenty). The dates in this listing are
  approximate and mostly placeholders: do not build per-year counts on them.
  Read a video's own page for its exact upload date.
- Requests are rate-limited per session. Download subtitles for specific long
  interviews; never loop over a whole channel.

## TikTok

- Video list with dates and plays:
  `yt-dlp --flat-playlist --print "%(upload_date)s %(view_count)s %(title)s" "https://www.tiktok.com/@<account>"`
  Captions in this listing are cut at 70 characters, so a count of posts
  carrying a tag or a mention is a floor. Plays are today's cumulative totals,
  and deleted or private videos are absent.

## X

- One post, or an account's current follower count and join date, without
  login: `https://api.fxtwitter.com/<handle>/status/<id>` and
  `https://api.fxtwitter.com/<handle>` (JSON).

## Follower history from the Wayback Machine

- List captures:
  `curl -s "https://web.archive.org/cdx?url=<profile url>&output=json&collapse=timestamp:6&fl=timestamp,statuscode"`
- The whole curve in one command, for every address the profile has had:
  `<this skill>/scripts/wayback.mjs curve <work>/raw/archive <address>...`
  It lists the monthly captures, fetches them in one batch, saves every page,
  and prints one JSON line per capture: date, the count it read (`value`), the
  page's own wording when the count is rounded or in another language
  (`text`), the capture URL and the saved file.
- Other captures in one batch, never one `curl` at a time: put the capture
  URLs (`https://web.archive.org/web/<timestamp>id_/<url>`) in a file, then
  `<this skill>/scripts/wayback.mjs fetch <work>/raw/archive --from <file>`.
- Both keep to 30 requests a minute and stop with an error when the archive
  refuses the connection: report that error, do not retry around it. Requests
  go through the proxy in `ISP_PROXY_URL` (one URL; the proxy rotates its exit
  addresses itself); without it they go direct. One agent at a time uses the
  archive.
- A status of 429 in the output is a capture of a page that answered 429 at
  the time, not a limit on you. Pick another capture near that date.
- Where the counts are:
  - Old and current YouTube channel pages (`/user/<name>`, `/channel/<id>`)
    carry "N subscribers".
  - Archived Social Blade pages carry followers, uploads and a daily table. The
    "last 30 days" box on those pages can be wrong; use the daily table.
  - Archived `twitter.com/<handle>` pages carry the follower count in the
    visible profile and in JSON-LD (`"name": "Follows", "userInteractionCount": N`).
    Some captures are empty shells; try neighbouring captures and
    `mobile.twitter.com`.
- Before writing that a period has no archive data, list the captures for every
  URL form the profile had.

## Records

- Securities filings: the regulator's own filing pages, fetched directly.
- Company registries: the registry itself. Sites that resell registry data are
  not the registry; say what they are.

## Audio with no transcript

Use the `transcribe` skill of this plugin
(`${CLAUDE_PLUGIN_ROOT}/skills/transcribe`). Prefer subtitles when the same
episode is on YouTube.
