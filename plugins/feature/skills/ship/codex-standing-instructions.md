Standing instructions:

- Work only in this directory, and change only the files your brief gives you: the
  workstream's `Files:` line, or in a fix round the files the findings name and the tests that
  cover them. Any other file belongs to a workstream running beside you, even a test or a helper that
  only covers your files. When the work cannot be done without one, ask with `ask_claude`.
- Commit after every coherent step. Never push. Leave no uncommitted change when you finish.
- Run the Checks commands before finishing and leave them green.
- Use `ask_claude` for a question the block does not answer instead of guessing. When no answer
  comes and you are told to proceed on your own judgment, record the assumption in `decisions`
  marked `(unconfirmed)`.
- Your last message is EXACTLY this flat JSON, with no prose around it:

  {"status": "done" | "blocked", "commits": [...], "decisions": [...], "findings": [...]}

  `commits` is the short SHAs you made, in order. `decisions` is one line per rule the block did
  not state and the code now follows: an answer you got from `ask_claude`, or an assumption of
  your own ending in `(unconfirmed)`. `findings` is one line per thing left open, each
  `path:line — claim`, with `path:line` left off when it has no place in the code: what you
  could not finish, a risk that remains, a place where the plan and the code disagree. `blocked`
  means the work is not finished; say why in `findings`.

  Shape only, never values to copy:

  {"status": "done", "commits": ["a1b2c3d"], "decisions": ["a full board spawns no food (answered by ask_claude)", "a write failure keeps the in-memory record and logs once (unconfirmed)"], "findings": ["lib/store.mjs:88 — the plan names a `flush()` the module never had; added it"]}
