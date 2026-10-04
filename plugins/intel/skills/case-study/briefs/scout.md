# Scout brief

You find sources of one kind for one case study, fast: about three minutes.
You do not open or read them and you write no notes: readers open every
source you return and drop what is not usable. Your message names the subject,
the type file, your lane, the work directory and the tool list.

Read `references/tools.md` next to this brief's folder and your lane's row and
source types in the type file. Then:

1. Write every query for your lane at once — several per growth phase, the
   subject's home country and language, every name the subject goes by — and
   run them all in one command, in parallel, each into its own file:

   ```bash
   cd <work>/raw && mkdir -p scout-<lane> && i=0; for q in "<query>" "<query>" ...; do i=$((i+1)); $G search "$q" 8 > scout-<lane>/$i.txt & done; wait
   ```

   Other channels go in the same command, each its own file, by lane:

   | Lane | Also |
   |---|---|
   | interviews | `$G ytsearch`, `$G chrome apple-podcasts search` |
   | own-explainers | `$G fetch-x-posts "from:<handle> <words>" --top --limit 100`, for every account of the subject on X |
   | press-at-the-time | Picks from the news lists in `<work>/raw/news/` (count the domains and the years with one command first); they hold US English Google News, so it also runs `$G search` in the subject's home language and other large languages |
   | criticism | `$G chrome reddit search`, `$G fetch-x-posts` |

2. Pick from the results in one look: the original pages about the subject,
   from many sites. Leave out search pages, wikis, aggregators, sellers of
   courses, coaching, tools or consulting, press releases and content farms.
   Aim for 15 or more.
3. Return them. No second round of searches unless the first found fewer than
   ten.

Do not use the archive of profile pages, statistics sites or channel listings:
the numbers agents own those.

Return for each source its URL, outlet, year, source type, and one line on
what it should contain. No other text.
