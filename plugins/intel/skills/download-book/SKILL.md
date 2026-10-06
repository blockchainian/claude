---
name: download-book
description: Search Anna's Archive for EPUB books, compare result download counts, extract fast and slow download links, and download the selected book to ~/Downloads. Use when asked to find a book's download options, inspect this site's EPUB search results, or download a book.
---

# Download Book

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

## Environment Variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env`, then fill in the member key. The script reads this file directly; it does not read the member key from the process environment.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ANNA_ARCHIVE_SECRET_KEY` | Anna’s Archive member key | Yes, for member fast-download links | `~/.config/intel/.env` |

## Setup (once)

Requires Node.js 20.12+ for `util.parseEnv`.

The site sits behind DDoS-Guard, which serves a captcha to headless browsers and to plain HTTP clients. The script therefore drives a headed Google Chrome window through Playwright: it needs Google Chrome installed and the `playwright` package next to the script. The same script launches its own headed Chrome in both hosts; it does not require host browser tools. Install dependencies beside that script:

```sh
npm install --prefix "$SKILL_DIR/scripts"
```

The browser profile persists at `~/.cache/secrets-manager/profiles/download-book`. One Chrome window serves every run: the first run opens it, each run works in a background tab of its own (it does not take the focus) and closes that tab when it finishes, and the window closes by itself a minute after the last tab. Several books can be looked up at once, one run per book; do not use the window meanwhile.

## Run

Run the bundled script with a book title and the file to save it to:

```sh
"$SKILL_DIR/scripts/anna-archive-links.mjs" "Pride and Prejudice" --out ~/Downloads/"Pride and Prejudice.epub"
```

The script searches the first EPUB results page, reads download counts embedded in that page (using the metadata endpoint only when a count is missing), selects the highest count, calls the fast download API, and resolves a slow download link. For the slow link it prefers a "slightly faster but with waitlist" server (these download at megabytes per second after a short queue) over the "no waitlist" servers (immediate but throttled to tens of KB/s), polling the waitlist entry until its direct link appears and falling back to a no-waitlist server if the queue does not clear in time. It prints JSON with the selected record, any links found and, with `--out`, the `download` result. Without `--out` it only finds the links. Finding the links takes about 90 seconds — the browser check plus the slow server's queue.

Confirm the selected `title` matches the book the user asked for. The script picks the highest download count on the results page, which can be a different book when the query is loose; if it mismatches, re-run with a tighter query (add the author, edition, or subtitle) before downloading.

## Download

Download the book to `~/Downloads` automatically, without asking the user to approve it: pass `--out ~/Downloads/"<Title>.epub"`, naming the file from the book title. Run it in the background; a slow download can take many minutes.

With `--out` the script tries `fast.url` first, then `slow.url`, then every other slow entry on the detail page (waitlist servers first), resolving each only when the link before it answered 429 ("too many downloads at the same time from the same IP"). Any other HTTP failure stops it. It writes through a `.part` file and keeps the file only when it is an EPUB (a Zip), so an HTML error page is never saved. The JSON's `download` holds `path`, the `url` used and the `refused` links, or an `error`. When every link answers 429, another download from this IP (often the user's own browser) is still running: report that and retry after it finishes.

To turn the downloaded EPUB into a Chinese PDF, pass it to the `translate` skill, which reads the EPUB directly.

If the browser check still fails (the script reports 未通过浏览器验证, usually a captcha), save the search results and selected detail page as HTML from your own browser, then pass `--search-html` and `--detail-html`. Pass `--slow-html` for a saved slow download page when its live entry is blocked. The saved slow page must refer to the selected MD5 record.

Report missing or blocked links as unavailable. Do not infer a direct file URL from an error page; only download the file URL the script actually returned.

Never download from libgen (libgen.li, libgen.rs or any other mirror), even when the fast API is out of downloads or a server fails: its files are often samples, early releases or cut-off downloads. When the fast API is out of downloads, use `slow.url`.
