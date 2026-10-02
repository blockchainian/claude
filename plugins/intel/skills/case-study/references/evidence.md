# Evidence rules

## Keeping your context small

Every tool result you pull in is re-read on every later step, so a large dump
is paid for dozens of times. Pull in only what you need:

- A chapter: `grep -n` for the sentences that name your source or hold the
  figure or quote you are checking, not the whole file. Read a whole chapter
  only when your task is to write or fix it.
- A page or transcript: search it for the passage you need (`grep -n -C 3`
  on the saved file, or the fetch tool's `| grep` / `| head -c 6000`), not the
  full text. Save the full text under `raw/` once and search that file.
- A listing or a log: `head`, `tail`, `wc -l` or a count, not the whole thing.
- Your own output file at restart: that one file, nothing else extra.

## Writing as you go

Every agent writes its output file while it works, by appending (`>>` or an
edit that adds lines), never by rewriting the whole file: a rewrite makes you
produce the whole file again each time. When your output file already exists
at start, you were interrupted: read that one file, nothing else extra, and
continue from the first item it does not cover.

Every agent in a case study — scout, reader, writer, reviewer, fixer — works
to these.

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

In the sourced draft (`md/`), the sentence itself shows which kind a claim is
and names its source. The book text (`book/`) drops the labels and the source
names, and carries the same distinction in ordinary wording:

| Kind | What it is |
|---|---|
| self-reported | The subject, or their staff, manager or company, said it — also when a newspaper prints it |
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
- In the sourced draft, a quotation translated into the study's language is
  followed at once by the source's own words in ⟦ ⟧:
  `“标题太长，观众消化不了”⟦If you make your video caption too long, it will be too much for people to digest⟧`.
  The words in ⟦ ⟧ are copied letter for letter, an omission inside them
  marked `…`: a script looks them up in the source's saved text, and the
  reviewer judges the translation against them. A quotation left in the
  source's language carries no ⟦ ⟧. The book never prints ⟦ ⟧.

## Privacy

Do not identify a pseudonymous or anonymous subject. Do not cite, link, or keep
in notes any page that prints a claimed legal name or personal details, and do
not print case numbers or other handles that lead straight to one.

## Reasoning chapters

The chapter that applies the findings (what a person could copy, and what a
product given with the task could copy) is reasoning. It opens by saying it is
reasoning, not a finding, and rests only on what the finding chapters
established. The product is never named, in the draft or in the book.
