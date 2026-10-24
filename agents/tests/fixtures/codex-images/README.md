# Codex prompt images

Real rollout records, relative to `~/.codex/sessions/`:

- `wrapped.jsonl`: `2026/08/12/rollout-2026-08-12T15-16-00-019ff4d3-fab8-7b70-92a3-e6e2609140b9.jsonl`, lines 9–10.
- `context.jsonl`: the same August rollout, line 6; injected AGENTS/environment content without `content_item_kinds`.
- `fallback.jsonl`: `2026/02/19/rollout-2026-02-19T11-55-57-019c7742-4365-72e2-b5b0-62eb8949cf64.jsonl`, lines 7–8.
- `data-url.jsonl`: `codex-samples/prompt-images.jsonl`, a real Codex user prompt image data URL. Its payload is the valid 24×24 PNG from `codex-samples/probe.png`.

The captured output image payloads are represented by a valid 24×24 PNG. Tests relocate recorded paths to temporary files for existence checks. The image-only test cuts the recorded opening tag, image and closing tag from `wrapped.jsonl`; no named image-only response record was found locally.

On disk each response message precedes its completed `UserMessage` event. `tail()` walks backward, so the completed event arrives first. The February response has no turn ID or wrapper path; pairing uses its exact prompt text. The August response and event have matching turn IDs and paths.
