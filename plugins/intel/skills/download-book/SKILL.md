---
name: download-book
description: Search Anna's Archive for EPUB books, compare result download counts, and extract fast and slow download links without downloading a file. Use when asked to find a book's download options or inspect this site's EPUB search results.
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

The script searches the first EPUB results page, reads download counts embedded in that page (using the metadata endpoint only when a count is missing), selects the highest count, calls the fast download API, and opens the slow download entry. It prints JSON with the selected record and any links found. It does not fetch the book file. A run takes about 40 seconds, most of it the browser check.

If the browser check still fails (the script reports 未通过浏览器验证, usually a captcha), save the search results and selected detail page as HTML from your own browser, then pass `--search-html` and `--detail-html`. Pass `--slow-html` for a saved slow download page when its live entry is blocked. The saved slow page must refer to the selected MD5 record. `ANNA_SECRET_KEY` optionally supplies a member key for the fast API; do not print its value. Calling that API with a valid key may use the member's quota.

Report missing or blocked links as unavailable. Do not infer a direct file URL from an error page or visit a returned file URL unless the user explicitly asks to download it.
