# Codex inner activity records

Extracted from real local Codex rollouts. Source paths below are relative to the Codex `sessions/` directory. Identity metadata, command output, patch diff, MCP result, and wrapper output bodies were omitted; activity fields and timestamps were retained. Review-mode details in the non-activity fixture were also omitted.

- `file-change.jsonl`: `2026/10/03/rollout-2026-10-03T17-51-35-01a10465-1340-71c0-a3e6-c74f8578f5bc.jsonl`, lines 84.
- `mcp.jsonl`: `2026/10/02/rollout-2026-10-02T11-17-37-01a0fdd6-07b0-7240-b5a5-5393c6b3fecd.jsonl`, lines 21.
- `image-view.jsonl`: `2026/10/06/rollout-2026-10-06T19-48-22-01a11443-1036-7fb3-823f-f5f7d878d498.jsonl`, lines 513.
- `subagent.jsonl`: `2026/10/05/rollout-2026-10-05T13-38-48-01a10dca-5e11-7581-b8d8-4bebe60a6afd.jsonl`, lines 133.
- `collaboration.jsonl`: `2026/10/05/rollout-2026-10-05T13-30-29-01a10da3-df29-7380-abe4-8c64f7ed5f8d_01a10dc2-c204-77e3-8f29-cc8a4904c947.jsonl`, lines 464.
- `command.jsonl`: `2026/10/09/rollout-2026-10-09T19-16-00-01a12398-8404-7780-ad37-01d689b680a3.jsonl`, lines 11, 14, 15.
- `late-command.jsonl`: `2026/10/13/rollout-2026-10-13T14-35-32-01a13731-2d91-7fd3-8c2a-4364ed9d8588.jsonl`, lines 11, 14, 31.
- `inline-array.jsonl`: `2026/10/13/rollout-2026-10-13T16-58-13-01a137b3-cf8f-7450-871a-bc7aff9f6d71.jsonl`, lines 276, 279, 280, 281.
- `extension.jsonl`: `2026/10/07/rollout-2026-10-07T16-55-42-01a118cb-58c9-7931-90c2-17d36e492b52.jsonl`, lines 100.
- `pure-js.jsonl`: `2026/10/05/rollout-2026-10-05T22-39-03-01a10fb8-f855-7851-89b3-57074554bbee.jsonl`, lines 1538, 1541.
- `non-activity.jsonl`: `2026/10/03/rollout-2026-10-03T15-54-53-01a103fa-3afd-7aa1-8925-96b46ceaa947.jsonl` line 10; `2026/10/03/rollout-2026-10-03T13-32-23-01a10377-c545-78a1-8a3b-842a994ffdd3.jsonl` line 22; `2026/10/03/rollout-2026-10-03T11-10-34-01a102f5-ef80-7b80-b383-fcf617823a00.jsonl` line 673; `2026/10/02/rollout-2026-10-02T19-49-08-01a0ffaa-551d-71f1-868b-1023d27526e9.jsonl` line 2; `2026/10/02/rollout-2026-10-02T19-49-08-01a0ffaa-551d-71f1-868b-1023d27526e9.jsonl` line 47.

- `file-change-without-start.jsonl`: `2026/06/12/rollout-2026-06-12T22-31-00-019ebf76-46e6-7da2-acdd-4676737d367e.jsonl`, lines 150–152. Patch path scrubbed; timing fields retained, including the absent start.
- `image-generation.jsonl`: `2026/07/21/rollout-2026-07-21T19-13-52-019f8461-d57c-7870-920d-fcac3f017a77.jsonl`, line 162. Image result omitted, prompt scrubbed, optional saved path omitted.

- `../codex-command-failed.jsonl`: command completion from the same source as `command.jsonl`, line 14. Command, output, path and identifiers anonymized; non-zero exit and timing retained.
