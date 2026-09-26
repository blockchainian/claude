---
name: download-book
description: Search Anna's Archive for EPUB books, compare result download counts, extract fast and slow download links, download the selected book to ~/Downloads, and convert it into a dark-themed English PDF. Use when asked to find a book's download options, inspect this site's EPUB search results, download a book, or turn a book into a PDF.
---

# Download Book

## Setup (once)

The site sits behind DDoS-Guard, which serves a captcha to headless browsers and to plain HTTP clients. The script therefore drives a headed Google Chrome window through Playwright: it needs Google Chrome installed and the `playwright` package next to the script. `${CLAUDE_PLUGIN_ROOT}` is the installed `intel` plugin root:

```sh
npm install --prefix "${CLAUDE_PLUGIN_ROOT}/skills/download-book/scripts"
```

The browser profile persists at `~/.cache/secrets-manager/profiles/download-book`. A Chrome window opens for the run and closes when the script finishes; do not use it meanwhile.

PDF conversion (see below) additionally needs Calibre (`ebook-convert`) and poppler (`pdftoppm`, `pdffonts`), plus the `pdf-lib` package that the `npm install` above pulls in:

```sh
brew install --cask calibre && brew install poppler
```

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

## Convert to PDF

Once the EPUB is downloaded and verified, convert it to a dark-themed English PDF automatically, without asking. Run the bundled script; it typesets the book with Calibre (Baskerville, 5.93×9.153 in page, clickable in-document TOC and chapter bookmarks), paints the `#000409` page background behind every page except the cover, and prints a JSON verdict.

```sh
node "${CLAUDE_PLUGIN_ROOT}/skills/download-book/scripts/epub_to_pdf.mjs" ~/Downloads/"<Title>.epub"
```

The PDF is written next to the EPUB (`<Title>.pdf`) unless a second path argument is given. Calibre takes a few minutes on a full book, so run it in the background and report the path when it finishes. The verdict's `ok` is true only when Baskerville is embedded, a sampled text page is mostly the dark background, the cover is not darkened, bookmarks exist, and the front TOC has working links; if `ok` is false, report the failing check rather than treating it as done.

The dark theme fits prose. A figure- or equation-heavy book (textbooks, math) keeps white boxes on those pages, because the EPUB's own figures have white backgrounds that the background paint sits behind, not over — note this to the user rather than trying to recolor figures.

If the browser check still fails (the script reports 未通过浏览器验证, usually a captcha), save the search results and selected detail page as HTML from your own browser, then pass `--search-html` and `--detail-html`. Pass `--slow-html` for a saved slow download page when its live entry is blocked. The saved slow page must refer to the selected MD5 record. `ANNA_SECRET_KEY` optionally supplies a member key for the fast API; do not print its value. Calling that API with a valid key may use the member's quota.

Report missing or blocked links as unavailable. Do not infer a direct file URL from an error page; only download the file URL the script actually returned.
