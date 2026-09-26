---
name: download-book
description: Search Anna's Archive for EPUB books, compare result download counts, extract fast and slow download links, and download the selected book to ~/Downloads. Use when asked to find a book's download options, inspect this site's EPUB search results, or download a book.
---

# Download Book

## Setup (once)

The site sits behind DDoS-Guard, which serves a captcha to headless browsers and to plain HTTP clients. The script therefore drives a headed Google Chrome window through Playwright: it needs Google Chrome installed and the `playwright` package next to the script. `${CLAUDE_PLUGIN_ROOT}` is the installed `intel` plugin root:

```sh
npm install --prefix "${CLAUDE_PLUGIN_ROOT}/skills/download-book/scripts"
```

The browser profile persists at `~/.cache/secrets-manager/profiles/download-book`. A Chrome window opens for the run and closes when the script finishes; do not use it meanwhile.

## Run

Run the bundled script with a book title:

```sh
node "${CLAUDE_PLUGIN_ROOT}/skills/download-book/scripts/anna_archive_links.mjs" "Pride and Prejudice"
```

The script searches the first EPUB results page, reads download counts embedded in that page (using the metadata endpoint only when a count is missing), selects the highest count, calls the fast download API, and opens the slow download entry. It prints JSON with the selected record and any links found. The script itself does not fetch the book file. A run takes about 40 seconds, most of it the browser check.

Confirm the selected `title` matches the book the user asked for. The script picks the highest download count on the results page, which can be a different book when the query is loose; if it mismatches, re-run with a tighter query (add the author, edition, or subtitle) before downloading.

## Download

Once a link is found, download the book to `~/Downloads` automatically, without asking the user to approve it. Prefer the `fast.url` when the fast API returned one; otherwise use `slow.url`. Name the file from the book title with a `.epub` extension. For example:

```sh
curl -fL --retry 2 -o ~/Downloads/"<Title>.epub" "<fast.url or slow.url>"
```

The slow host is often slow (tens of KB/s); run the download in the background and report the saved path when it finishes. Verify the result is an EPUB (`file` reports a Zip/EPUB, not HTML) — an HTML result means the link was an error or captcha page, which is unavailable.

If the browser check still fails (the script reports 未通过浏览器验证, usually a captcha), save the search results and selected detail page as HTML from your own browser, then pass `--search-html` and `--detail-html`. Pass `--slow-html` for a saved slow download page when its live entry is blocked. The saved slow page must refer to the selected MD5 record. `ANNA_SECRET_KEY` optionally supplies a member key for the fast API; do not print its value. Calling that API with a valid key may use the member's quota.

Report missing or blocked links as unavailable. Do not infer a direct file URL from an error page; only download the file URL the script actually returned.
