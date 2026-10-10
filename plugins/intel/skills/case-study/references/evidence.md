# Evidence rules

Every agent in a case study (scout, reader, numbers agent, writer) works to
these.

## Keeping your context small

Every tool result you pull in is re-read on every later step. Pull in only
what you need:

- A chapter: `grep -n` for the sentences that name your source or hold the
  figure or quote you are checking. Read a whole chapter only when your task
  is to write it.
- A page or transcript: save the full text under `raw/` once and search it for
  the passage you need (`grep -n -C 3` on the saved file, or the fetch tool's
  `| grep` / `| head -c 6000`).
- A listing or a log: `head`, `tail`, `wc -l` or a count.

## Writing as you go

Write your output file while you work, by appending (`>>` or an edit that adds
lines), never by rewriting the whole file. When your output file already
exists at start, you were interrupted: read that one file, nothing else, and
continue from the first item it does not cover.

## What counts as read

- A source counts only when you opened it and read it. A search snippet, a
  headline, or another page's summary of it is not a read.
- A fact known only through a repost, an aggregator, Wikipedia, or a later
  article that cites the original is a lead. Open the original, or the fact
  does not go in.
- A page read only up to a paywall supports only what the readable part says.
- A blocked or empty fetch is "could not reach", not "does not exist". Record
  it as a gap with the command tried and what came back.
- A verbatim download of a source saved in `raw/` (subtitles, a PDF, an
  archived page's HTML) counts as the source. Notes about a source do not.

## The kind of every claim

In a chapter (`drafts/`), a bracket in the sentence names its source and the
kind of claim. The book text (`book/`) is the chapter without the brackets, so
the sentence carries the same distinction in ordinary wording too:

| Kind | What it is |
|---|---|
| self-reported | The subject, or their staff, manager or company, said it, also when a newspaper prints it |
| on record | An archive snapshot, a platform page, a filing, a court document |
| reported at the time | Press published when the event happened |
| reported later | A profile or retrospective written afterwards |

- The subject's own story of how they succeeded is a claim. Collect all of it,
  in detail, labelled as theirs.
- A later profile does not prove what was known earlier.
- An estimate (a magazine's earnings list, a data firm's model) is called an
  estimate. Unaudited decks and pitch material are not "actual figures".
- Accusations against a named third party are worded as the accuser's
  allegation or as a court's or official body's finding, with the outcome when
  one is known.

## Who cannot be evidence

- Anyone who earns from telling success stories or teaching growth or money:
  sellers of courses, coaching, cohorts, paid communities, growth or creator
  tools, or consulting on going viral. Their "how X succeeded" pieces are leads
  to original sources, nothing more.
- Sponsored and paid write-ups, press releases and PR wires, sponsor-written
  narration, SEO content farms, AI-written wikis, Wikipedia.
- An interview hosted by a seller: the host's claims and framing are out. The
  subject's own words on record there may stay, labelled self-reported, with
  the sentence naming the host as someone who sells courses or consulting.
- Check how a publisher earns on the site itself (pricing, "work with me",
  course links), not from memory.

## Interested parties

A manager, agent, employer, investor, sponsor, the platform's own PR, and an
outlet owned, founded or funded by the subject or their staff all have a stake.
Their statements are self-reported, and the tie is stated in the sentence where
the source is used. A piece from a publication's outside-contributor network is
not that publication's reporting — name it as a contributor piece.

## Numbers

- Followers, views, uploads, revenue, deal sizes, dates of milestones: check
  each against an archive snapshot, a platform record, or press from the time.
- When the subject's account and the record differ, print both. When two
  sources differ, print both.
- A derived figure (a growth rate, a duration, a per-period count) states what
  it was computed from, and both ends come from the same kind of record.
- Never invent a person, a number, a quote, or a URL. What cannot be found is
  written as "not found".

## Quotes

- Keep quotes short. A translated quote is a faithful translation of words that
  are in the source; anything inside quotation marks must be findable there,
  from the stated speaker, on the stated date, in the stated outlet.
- Quotation marks hold a source's words and nothing else: none around a term,
  a heading or a phrase of your own.

## Privacy

Do not identify a pseudonymous or anonymous subject. Do not cite, link, or keep
in notes any page that prints a claimed legal name or personal details, and do
not print case numbers or other handles that lead straight to one.

## Reasoning chapters

The chapter that applies the findings (what a person could copy, and what a
product given with the task could copy) is reasoning. It opens by saying it is
reasoning, not a finding, and rests only on what the finding chapters
established. The product is never named.
