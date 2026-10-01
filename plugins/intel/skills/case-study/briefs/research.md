# Research brief

You are the research agent for one case study. Your deliverable is a set of
files in the work directory, written as you go. Your final message is a short
coverage summary and nothing else.

The values for this run — subject, subject type file, work directory, language,
tool list, and the product for the last reasoning chapter (if any) — are in the
message that sent you here.

## Read first

1. `references/evidence.md` next to this brief's folder — the rules every
   sentence must meet.
2. The subject type file — source types to cover, what the numbers must
   establish, the chapter list.
3. `references/tools.md`, and the user's tool list if one was given.

## Target

- Dozens of sources opened and read: aim for 60 or more, from 30 or more
  different sites, across every source type the type file lists. Archive
  snapshots are counted separately from sources.
- Every chapter in the type file's list. Each has a lead paragraph under its
  title, then five or six `##` sections.
- Depth over padding: a method is written out in full (what exactly, how often,
  with whom, at what cost, what changed), with the number, its source and its
  year inside the sentence. Fewer solid claims beat more weak ones.

## Files, all inside the work directory

- `raw/` — every download: subtitles, transcripts, PDFs, archived pages.
- `notes.md` — append after every source you open: URL, outlet, date, how you
  read it and how much of it, then the facts and short quotes you took, each
  tagged with its kind. Your context will be compacted during this job; this
  file is your memory. Write to it before moving on.
- `sources.json` — an object mapping `url` to `"Outlet Year"`. Add a URL the
  moment you have opened and read it. Only sources in this file may be named in
  the chapters, and every source named in the chapters is in this file.
- `gaps.md` — every source you tried and could not read, with the command tried
  and what came back; and every question the record did not answer.
- `md/01.md` onward — the chapters. First line `# <chapter title>`, then the
  lead paragraph, then `##` sections. Do not edit `chapters.json`.

## Order of work

1. Open and read the starting sources the message gave you, if any.
2. Widen by source type, one type at a time. For press from the time, search
   each growth phase with a date range.
3. Build the dated record first: the curve, the upload record, the milestones.
   List the archive's captures before fetching, and fetch through to today.
4. Write the chapters from `notes.md`.
5. Before finishing, re-read your own chapters against these, the failures an
   unreviewed study usually has:
   - a source named in the text that you never opened (it came through
     Wikipedia, a repost, or another article's citation);
   - a seller's, manager's, sponsor's or platform PR's statement written as an
     independent fact;
   - words inside quotation marks that are not in the source, or belong to the
     interviewer;
   - a "conflict between the subject and the record" that comes from a
     reposting site splicing the original — open the original interview;
   - "no archive data for this period" written without listing the captures;
   - a growth step credited to an event without dated points on both sides;
   - a ratio or rate computed from two figures of different dates or kinds;
   - sources listed in `sources.json` that no chapter uses.

## Shared machine

Other agents may be running. Keep to the request spacing in the tools file,
run waiting commands in the foreground with a bounded time, and stay within any
request caps the message gave you.

## Final message

Counts only: sources read (archive snapshots apart), distinct sites, sources by
type; chapters written with their lengths; the curve points you verified from
the record; the main conflicts between self-report and record; the main gaps.
No chapter text.
