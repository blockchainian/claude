# <topic>: problem statement

Status: problem statement for a planning agent; no design decided.
Base: `<short sha>` on branch `<branch>`, <date>.

## The ask

"<the user's words, verbatim>"

Decision in one sentence: <what is being changed and why now>.

## Facts

One bullet per fact, evidence inline. Code as `path.ts:120`; measurements as
the command and its dated output; wire shapes as a real response body.

- <fact> — `path/file.ts:NN`
- <measurement> — `<command>`, <date>:

  ```
  <real output, trimmed>
  ```

- <wire field> — real body from `<producer endpoint>`, <date>:

  ```json
  { }
  ```

## Checks (the gate)

The project's per-module gate, transcribed from its AGENTS.md "Checks (the gate)" section — the
plan's `Checks` command composes these (type/compile check + lint + tests, path-filtered per
touched module; full and unfiltered post-merge). Transcribe the real commands, do not invent them.
"none declared" if the project's AGENTS.md has no such section.

- <module>: type-check `<cmd>`, lint `<cmd>`, tests `<cmd> <path>`

## Open questions

Candidate mechanisms as questions, each with what would settle it. No proposals.

- <question>? Settled by <probe / query / request>.

## Unverified

Facts wanted and not confirmed, one per line, with what would settle it.
"none" if empty.

- <fact> — settle by <how>.
