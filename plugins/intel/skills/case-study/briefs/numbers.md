# Numbers brief

You build the dated record for a case study from platform records, not from
anyone's telling. Read "Numbers" in the evidence rules, the tool list, and
"What the numbers must establish" in the type file.

## Workstream: follower-numbers

Your notes are named `numbers-followers`. You are the only agent that fetches
from the archive.

1. Run the curve once for every address the profile has had (old user names,
   channel ids, handles, and the statistics-site page for the account),
   through the gate as the tool list says:
   `$G wayback curve <work>/raw/archive <address>...`
   It prints one line per capture with the count it could read; do not fetch
   captures one by one. When it exits non-zero naming lists or captures the
   archive did not give, work from the rows it printed and write what is
   missing into the gaps as it is.
2. A row with `text` but no `value` shows a rounded or foreign-language
   count: read the text. For a row with neither, open the saved file and
   look; drop a capture that is a redirect or an error page.
3. From the rows, write the curve (at least one point per quarter in the
   growth years and one per half-year after, through today), today's count
   read today, the dated milestones (the first capture at or above each
   threshold, and the last one below it), and the fastest stretch between two
   points of the same kind. Statistics-site captures often carry a daily
   table: use it for the exact milestone days.

## Workstream: posting-numbers

Your notes are named `numbers-posting`.

List the account's uploads or posts with the platform command in the tool
list, which lists a whole account with exact dates: run it once, save its
output under `raw/`, and count from that file. Write the earliest posts
(date, title, views), the count per month in each phase, where the cadence or
format visibly changed, and what was posted in the weeks around each
acceleration the press mentions.

## Output, both workstreams

Append to `notes/<name>.md` as you go, `<name>` being your workstream's notes name
(see "Writing as you go" in the evidence rules). Write dated tables, one row
per line, each row starting with the chapter tags it serves (`[c03]`,
`[c07]`…) and `[on record]`, and ending with the record's URL. When the workstream
is done, write three more files in exactly these shapes, since the merge
drops any other:

- `notes/<name>.sources.json`: one JSON object, `{"<url>": "<label>"}`, for
  every record you cite, the URL exactly as the rows print it and the label
  as the chapters will name it (`Internet Archive 2019-10-01
  youtube.com/channel/…`, `YouTube video page 2026`);
- `notes/<name>.raw.json`: one JSON object, `{"<url>": ["raw/<file>", …]}`,
  giving for each of those URLs the files its page was saved to, as paths
  from the work directory (`wayback.mjs` prints the file as `file` next to
  each `url`);
- `notes/<name>.gaps.md`: one line per record you could not read. A period
  with no captures is a gap only after the full capture list shows none.

Final message: points on the curve, first and last date, milestones found,
gaps. Nothing else.
