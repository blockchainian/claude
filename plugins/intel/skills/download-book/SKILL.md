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

The script needs Node.js 20.12+ and Google Chrome. Install its dependencies once:

```sh
npm install --prefix "$SKILL_DIR/scripts"
```

While a run is going, leave alone the headed Chrome window the script opens.

## Run

```sh
"$SKILL_DIR/scripts/anna-archive-links.mjs" "Pride and Prejudice" --download
```

The script takes the most-downloaded EPUB on the first search page and prints JSON with its `title`,
`md5`, `fast` and `slow` links and, with `--download`, either `download.path` or `download.error`.
Download without asking the user, in the background and one book at a time, because this IP allows
only one download at once; looking up links without `--download` takes about 90 seconds and can run
in parallel.

Check that `title` is the book the user asked for; if it is not, re-run with the author, edition or
subtitle added. Then list the EPUB with `unzip -l` to confirm it holds real XHTML chapters rather
than one image per page. For a math book, reject an O'Reilly EPUB whose files sit under
`sbo-rt-content/`, because its MathML is broken.

When every link is refused with 429, another download from this IP, often in the user's own
browser, is still running: say so and retry once it ends. When the script reports 未通过浏览器验证, a
captcha blocked it. Save the search page and the selected detail page as HTML from your own browser
and pass them with `--search-html` and `--detail-html`, adding `--slow-html` with a saved
slow-download page of the same MD5 if that page is blocked too. Use only file URLs the script
returned, and report missing links as unavailable. Never download from libgen or its mirrors, whose
files are often samples, early releases or truncated.

To get a Chinese PDF, pass the EPUB to the `translate` skill.
