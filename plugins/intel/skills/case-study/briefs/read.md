# Reader brief

You read one batch of sources for a case study and write notes. Chapter
writers will see only the notes, never the sources, so the notes carry
everything usable. Your message names the subject, the type file, the work
directory, your batch name, your URLs, and your share of the machine's caps.

Read `references/evidence.md` and `references/tools.md` next to this brief's
folder, and the chapter table in the type file.

Start with this command, in a call of its own, before any fetch:
`<skill>/scripts/case-study.mjs unread <work> <batch> <url>...` with your
URLs (the skill folder is the one that holds this brief's folder). It prints
one line per source: `fetch` (not opened yet), `saved` with the files its text
is already in (read those files; do not fetch it again), or `done` (its notes
are written; skip it).

Take the sources one at a time, each through all three steps before the next
is opened.

1. Open it and save the full text (the page, the transcript, the PDF's text)
   as a file under `raw/<batch>/`. The study's figures are checked against
   these files.
2. Append one line to `notes/<batch>.raw.tsv` for each file saved: the URL, a
   tab, the file's path from the work directory (`raw/<batch>/<file>`).
3. Read the saved text to the end (a long interview or transcript too) and
   append the source's section to `notes/<batch>.md`:

```
## <Outlet> — <title> (<date>)
url: <url>
read: <command>, full | partial (<how much>)
publisher: <who, how they earn, any tie to the subject>
- [c04][c06] [self-reported] (2019-03) <fact, method detail, figure or short quote in the original wording, with the speaker> — <Outlet Year>
```

- One bullet per fact, on one line. Each starts with the chapter files it
  serves (`[c02]` … from the type file's table), then its kind
  (`[self-reported]`, `[on record]`, `[reported at the time]`,
  `[reported later]`), then the date the fact refers to. Each ends with the
  source label.
- Write methods out in full: what exactly, how often, with whom, at what cost,
  what changed. Keep the original wording for anything specific.
- A source that turns out to be a seller, a press release, a repost or a wiki
  gets its heading, its `url:` line and one line saying so, and no bullets.
  Follow a repost to its original and read that instead, as a source of its
  own.

When the batch is done, write:
- `notes/<batch>.sources.json` — `{"<url>": "<Outlet Year>"}` for every source
  you opened and read (full or partial), and no others;
- `notes/<batch>.gaps.md` — one line per source you could not read: the
  command tried and what came back.

Write only these four files (the notes, the list of saved files, the sources
and the gaps) and `raw/<batch>/`. Final message: sources read, partial,
not reached. Nothing else.
