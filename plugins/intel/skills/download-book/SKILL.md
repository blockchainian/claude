---
name: download-book
description: Search Anna's Archive for EPUB books, compare result download counts, extract fast and slow download links, and download the selected book to ~/Documents/books. Use when asked to find a book's download options, inspect this site's EPUB search results, or download a book.
---

# Download book

## Runtime and paths

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Setup (once)

The script needs Node.js 20.12+, Google Chrome, and its npm dependencies:

```sh
npm install --prefix "$SKILL_DIR/scripts"
```

It fetches the site through a headed Chrome window that it opens itself, one background tab per run, so several lookups can run at once. Do not use that window while runs are in progress.

## Environment variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env` and fill in the member key there; the script reads only that file.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ANNA_ARCHIVE_SECRET_KEY` | Anna’s Archive member key | Yes, for member fast-download links | `~/.config/intel/.env` |

## Run

Pass a book title and the file to save it to:

```sh
"$SKILL_DIR/scripts/anna-archive-links.mjs" "Pride and Prejudice" --out ~/Documents/books/"Pride and Prejudice.epub"
```

The script picks the EPUB result with the most downloads on the first search page and prints JSON with that record (`title`, `md5`, `downloads_total`), its `fast` and `slow` links and, with `--out`, the `download` result. Without `--out` it only finds the links, which takes about 90 seconds.

Confirm the selected `title` matches the book the user asked for. A loose query can make a different book the most downloaded; re-run with a tighter query (add the author, edition, or subtitle) before downloading. The top record can still be a fan conversion or page scans: once downloaded, list the EPUB's entries (`unzip -l`) and check it holds real XHTML chapters, not one image per page. For a math book, avoid O'Reilly EPUBs whose files sit under `sbo-rt-content/`: their MathML is broken.

## Download

Download the book to `~/Documents/books` (`<INTEL_OUTPUT_DIR>/books` when set) without asking the user to approve it, passing `--out ~/Documents/books/"<Title>.epub"` named from the book title. Run it in the background, since a slow download can take many minutes. Download several books one at a time, because this IP allows only one download at a time; finding links without `--out` can run in parallel.

The JSON's `download` holds the saved `path`, the `url` used and the `refused` links, or an `error`. When every link was refused with 429, another download from this IP (often the user's own browser) is still running: report that and retry after it finishes.

If the browser check fails (the script reports 未通过浏览器验证, usually a captcha), save the search results and the selected detail page as HTML from your own browser and pass them with `--search-html` and `--detail-html`. Pass `--slow-html` for a saved slow download page when its live entry is blocked; that page must belong to the selected MD5 record.

Report missing or blocked links as unavailable, and download only a file URL the script returned, never one inferred from an error page. Never download from libgen or any of its mirrors, even when the fast API is out of downloads or a server fails: its files are often samples, early releases or cut-off downloads.

To turn the EPUB into a Chinese PDF, pass it to the `translate` skill.
