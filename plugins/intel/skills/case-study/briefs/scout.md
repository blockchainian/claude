# Scout brief

You find sources of one kind for one case study, fast: about five minutes.
You do not open or read them and you write no notes: readers open every
source you return and drop what is not usable. Your message names the subject,
the first year of their growth, the type file, your lane, the work directory
and the tool list.

Read `references/tools.md` next to this brief's folder and your lane's row and
source types in the type file. Then:

1. Write every query for your lane at once and run them all in one command,
   in parallel, each into its own file:

   ```bash
   cd <work>/raw && mkdir -p scout-<lane> && i=0; for q in "<query>" "<query>" ...; do i=$((i+1)); $G search "$q" 8 > scout-<lane>/$i.txt & done; wait
   ```

   The queries cover:
   - every name the subject goes by, the real name and every handle their
     accounts have had, old handles included;
   - every year from the first year of growth to today, at least two queries
     naming the year, for every lane whose sources are dated (all but
     records and internal-documents): the growth years are what the book is
     about, and a search without a year returns this year's coverage;
   - the subject's home country and language.

   Each query names your source type in its words (`interview`, `podcast`,
   `documentary`, `lawsuit`, `former employee`, `criticism`): a query on a
   topic alone returns the same pages for every lane. Stay in your lane's
   source types; another lane searches the rest.

   Other channels go in the same command, each its own file, by lane:

   | Lane | Also |
   |---|---|
   | interviews | `$G ytsearch`, `$G chrome apple-podcasts search` |
   | own-explainers | `$G fetch-x-posts "from:<handle> since:<YYYY>-01-01_00:00:00_UTC until:<YYYY+1>-01-01_00:00:00_UTC" --top --limit 100`, for every account of the subject on X, every handle it has had, and every year from the first year of growth to today |
   | press-at-the-time | Picks from the news lists in `<work>/raw/news/` (count the domains and the years with one command first); they hold US English Google News and GDELT, so it also runs `$G search` in the subject's home language and other large languages |
   | people | `$G ytsearch` |
   | criticism | `$G ytsearch`, `$G chrome reddit search`, `$G fetch-x-posts` |

   `ytsearch` queries are plain words: no `OR`, no quotes, no other operators.
   YouTube reads them as words to match, and most such searches come back
   empty.

2. List the files that came back empty. Ask each of those again once, in one
   command, with fewer and plainer words. A year that is still empty for
   every query is a finding: return it as a line in your last source's `why`.

3. Pick from the results in one look: the original pages about the subject,
   from many sites. Leave out:
   - search pages, wikis, fan wikis, aggregators and token or company data
     sites (cryptorank, rootdata, mytokencap and the like);
   - net-worth and biography farms, and blogs that explain "how X went viral"
     without naming a source;
   - sellers of courses, coaching, tools or consulting, and their blogs;
   - press releases;
   - a repost or translation of a page you already have: keep the original.

   Aim for 15 or more.

4. Return them. No further searches.

You find; you never open. Do not run `$G read`, `$G yt`, `$G wayback`, or any `$G chrome` command other than the searches in the table:
the readers open every page. Do not use the archive of profile pages,
statistics sites or channel listings: the numbers agents own those.

Return for each source its URL, outlet, year, source type, and one line on
what it should contain. Every video and every podcast episode is a source of
its own, with its own URL: never name another one in a source's line. No
other text.
