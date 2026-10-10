---
name: download-book
description: Search Anna's Archive for EPUB books, compare result download counts, extract fast and slow download links, and download the selected book to the intel output folder. Use when asked to find a book's download options, inspect this site's EPUB search results, or download a book.
---

# Download book

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ANNA_ARCHIVE_SECRET_KEY` | Anna’s Archive member key | Yes, for member fast-download links | `~/.config/intel/.env` |
| `INTEL_OUTPUT_DIR` | Output root; books go under `books/`; default `~/Documents` | Optional | `~/.config/intel/.env` |

## Setup

Needs Node.js 20.12+ and Google Chrome:

```sh
npm install --prefix "$SKILL_DIR/scripts"
```

The script drives a headed Chrome window it opens itself; don't use that window while a run is going.

## Run

```sh
"$SKILL_DIR/scripts/anna-archive-links.mjs" "Pride and Prejudice" --download
```

It picks the most-downloaded EPUB on the first search page and prints JSON: the record's `title`
and `md5`, its `fast` and `slow` links and, with `--download`, the saved `download.path` or
`download.error`. Download without asking the user, in the background, one book at a time: this IP
allows one download at once. Finding links alone (no `--download`, about 90 s) can run in parallel.

- Check `title` is the requested book; if not, tighten the query (author, edition, subtitle) and
  re-run.
- Check the EPUB holds real XHTML chapters (`unzip -l`), not one image per page. For a math book,
  reject O'Reilly EPUBs with files under `sbo-rt-content/`: their MathML is broken.
- Every link refused with 429: another download from this IP (often the user's browser) is
  running; say so and retry after it ends.
- 未通过浏览器验证 (a captcha): save the search page and the selected detail page as HTML from your
  own browser and pass `--search-html` and `--detail-html`; `--slow-html` takes a saved
  slow-download page of the same MD5.
- Use only file URLs the script returned, and report missing links as unavailable. Never download
  from libgen or its mirrors: their files are often samples, early releases or truncated.

Pass the EPUB to the `translate` skill for a Chinese PDF.
