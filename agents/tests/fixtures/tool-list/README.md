# Tool list records

Claude assistant tool calls captured in the example.com project:

- `0661ce7b-dc8a-4703-abea-7572bd880b12.jsonl:2333`: Read.
- `594feebb-ddc6-4bbc-bd61-3f544e1d6ce3.jsonl:534`: Write.
- `claude-tool-calls.jsonl:5`: captured Edit, reused from the existing fixture.
- `85c538ad-768f-4878-affd-665e7c4ed1f0.jsonl:299-300`: two Edit calls sharing one message ID.

Envelopes retain only identity, timestamps, project and tool blocks. Project paths
use `/workspace`; write and edit bodies retain their first line. Tests vary paths
and reuse the captured Read block with the Edit message ID for mixed grouping.
Codex coverage reuses `codex-inner/git-chain.jsonl` and `codex-tool-calls.jsonl`.

Codex completed items captured from:

- `2026/10/17/rollout-2026-10-17T20-50-29-01a14d21-e3db-71f3-ac08-d7dc18682d8a.jsonl:13`: read command.
- `2026/10/13/rollout-2026-10-13T18-52-03-01a1381c-06e5-7743-9a95-fb1f47beb709.jsonl:291`: two-file update.
- `2026/10/18/rollout-2026-10-18T12-46-13-01a1508c-e69a-73c0-923b-792e07f29aa3.jsonl:86`: file creation.

Project and home paths use `/workspace` and `/home/example-user`. Output/content retain
the first line; unified diffs retain their first hunk header and first changed lines.
