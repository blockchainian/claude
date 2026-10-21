# Refresh input latency measurements

Real local HOME, 200 × 50 PTY. Navigation only; no prompt submissions. No tmux server was created.

Each action follows a 300 ms PTY output drain. First-byte timing is write → the first PTY read that brings cumulative response above six bytes, ignoring a possible lone hide-cursor sequence; timestamps include terminal decoding. It does not identify the input-acknowledging frame. Burst-settle timing is write → the last observed PTY output before 60 ms without output, with a 2.5 s observation limit. These are different endpoints.

Percentiles use nearest rank. All 150 actions remain included, including any observation-limit tail. Keyboard, click and wheel groups use their full respective 60 / 30 / 60 actions.

| Metric | Historical report | baseline | after |
|---|---|---|---|
| First footer frame, ms | first content: 1851–1874 | 2156.48, 2038.26, 3260.84 | 359.02, 60.79, 53.58 |
| First visible session row, ms | not separately measured | 2152.12, 2034.27, 3255.81 | 1555.08, 1274.69, 1252.73 |
| First Active/Inactive header, ms | 1851–1874 | 2151.43, 2033.71, 3255.11 | 355.96, 59.01, 51.67 |
| Key/mouse burst settle p50, ms | under 10 | 38.19 | 48.24 |
| Key/mouse burst settle p80, ms | 662 | 69.68 | 70.3 |
| Key/mouse burst settle p99, ms | 951 | 692.71 | 303.08 |
| Keyboard p50 / p80 / p99, ms | not separately reported | 38.16 / 67.03 / 828.48 | 50.46 / 72.78 / 303.08 |
| Click p50 / p80 / p99, ms | not separately reported | 39.78 / 71.25 / 692.71 | 50.05 / 70.30 / 141.40 |
| Wheel p50 / p80 / p99, ms | not separately reported | 37.26 / 69.55 / 680.23 | 45.84 / 66.61 / 2502.12 |
| First byte p50 / p80 / p99, ms | not reported | 34.03 / 64.24 / 678.94 | 38.37 / 58.87 / 83.01 |
| Navigation actions with response | not given | 150 | 150 |
| Navigation actions without response | not given | 0 | 0 |
| Ctrl+C during startup window, ms | not measured | 1215.86, 1115.66, 1113.85 | 105.46, 103.81, 102.84 |
| Ctrl+C after interaction, ms | not measured | 109.12 | 110.35 |

## baseline

- Binary: `/private/tmp/agents-refresh-baseline-target/release/agents`.
- SHA256: `eb62ecdf04265520d6513c6f76a197bc09daab040df00b49e06ce065c1b4ec09`.
- Explicit archive source HEAD: `9af4e1ee35473c94e6441de5e792f32ddab09c74`.
- Provider/home overrides: `{"CLAUDE_CONFIG_DIR": null, "CODEX_HOME": null, "CODEX_MANAGER_STATE_DIR": null}`.

| Action | n | p50 ms | p80 ms | p99 ms |
|---|---:|---:|---:|---:|
| key-task-down | 30 | 40.74 | 63.86 | 828.48 |
| key-task-up | 30 | 36.64 | 67.03 | 634.33 |
| click-task | 30 | 39.78 | 71.25 | 692.71 |
| wheel-task-down | 30 | 38.03 | 54.46 | 680.23 |
| wheel-task-up | 30 | 36.28 | 69.55 | 575.0 |

- Click strategy: different visible leaf task; every target differs from the selected cursor row. Click coordinates and target text are saved in the interaction summary.
- Startup interrupt sample evidence:
  - Run 1: refresh observed; main thread=True, agents-refresh worker=False; sample return code=0.
  - Run 2: refresh observed; main thread=True, agents-refresh worker=False; sample return code=0.
  - Run 3: refresh observed; main thread=True, agents-refresh worker=False; sample return code=0.
- Source: unmodified archive SHA256 `25eabe21c0e569e77e84196a61c42f0f391de258ddd768a064289232a67a7fde`; provenance `baseline-source.json`.

## after

- Binary: `/Users/example-user/Code/.worktrees/agents-panel/agents/scratchpad/perf/fix-refresh/after-agents`.
- SHA256: `05590f3efda8e66397296748849212906642c9f215000845ea85f7dff63d71c7`.
- Explicit archive source HEAD: `9af4e1ee35473c94e6441de5e792f32ddab09c74`.
- Provider/home overrides: `{"CLAUDE_CONFIG_DIR": null, "CODEX_HOME": null, "CODEX_MANAGER_STATE_DIR": null}`.

| Action | n | p50 ms | p80 ms | p99 ms |
|---|---:|---:|---:|---:|
| key-task-down | 30 | 43.38 | 57.85 | 303.08 |
| key-task-up | 30 | 55.57 | 75.86 | 164.46 |
| click-task | 30 | 50.05 | 70.3 | 141.4 |
| wheel-task-down | 30 | 38.55 | 62.37 | 138.14 |
| wheel-task-up | 30 | 45.98 | 71.8 | 2502.12 |

- Observation-limit tail retained: action 150 (wheel-task-up) first response 1.99 ms, last observed output 2502.12 ms, 20855 bytes. The probe reached its 2.5 s limit before observing 60 ms quiet. This is a bounded observation, not a measured completed settle or proof of blocked input. No per-action stack/frame evidence identifies the output source.

- Click strategy: different visible leaf task; every target differs from the selected cursor row. Click coordinates and target text are saved in the interaction summary.
- Startup interrupt sample evidence:
  - Run 1: refresh observed; main thread=False, agents-refresh worker=True; sample return code=0.
  - Run 2: refresh observed; main thread=False, agents-refresh worker=True; sample return code=0.
  - Run 3: refresh observed; main thread=False, agents-refresh worker=True; sample return code=0.
- Source: the same frozen archive plus own patch SHA256 `09a31248bc6a2d2efcfa38b59c3f2fcb6781ce9258b76dbd21a8f7cb6458b8cb`; provenance `after-source.json`.

## Limits

- Historical numbers came from another source/data state and a broader interaction workload. The current before/after workload uses the same navigation actions.
- PTY byte timing can include unrelated refresh output; it does not identify the precise frame acknowledging a key. Actions without substantive output are counted separately and excluded from percentiles.
- Active/Inactive header presence measures a frame, not loaded session rows. The interaction mode waits after the footer and rejects a screen with no selected task.
- Startup interrupt timing alone does not prove the refresh was running at the exact key timestamp. Optional macOS sample records refresh in an overlapping window; missing/failed samples must be reported as inconclusive.
- Live writers and concurrent system load affect measurements. The explicit source SHA and binary checksum are recorded; source patch and archive construction evidence must accompany this report.


## Refresh blocked by real SQLite lock

The regression `refresh_allows_first_frame_input_and_exit_while_sqlite_is_locked` uses a real SQLite connection and `BEGIN EXCLUSIVE` with an 800 ms release timeout. It starts refresh, draws, handles Ctrl+C and drops App; the measured path must finish within 300 ms while the database remains exclusively locked and the refresh result has not been awaited. The test failed before with `895.717417ms` (`red.log`) and passes in the captured 24-test focused run (`candidate-final-host.log`, whole test duration 0.051 s). Whole-test duration is not a separate input latency measurement.

This deterministic lock regression complements the host PTY exit samples. If a fast exit prevents macOS sample from attaching or recording refresh, the corresponding host run does not prove refresh was in flight at the precise Ctrl+C timestamp.

## Source and exclusions

The comparison binaries both start from frozen source 9af4e1ee35473c94e6441de5e792f32ddab09c74. This performance comparison covers that archive plus the recorded own patch, separately from any later integration gates on a newer HEAD.

Excluded data: sandbox-failed/, unverified before-agents runs, script-smoke*/, candidate-* and baseline-interaction-fixed-coordinate-summary.json. They are retained only as probe diagnostics. No tmux server was created.

## Implementation

- `agents-refresh` owns a separate Hub and its provider caches. `Input::Sessions` returns the completed snapshot to the UI loop. At most one refresh runs; triggers during it merge into one subsequent request. Shutdown does not join the reader.
- Filesystem notifications retain their paths through the 50 ms debounce. Exact transcript matches take precedence over ancestor directories; matching sessions and their descendants refresh. Other rows retain their prior details. Unattributed paths and the five-second liveness deadline request a full snapshot. Discovery retains the existing history-page budget, replacement rows and child traversal.
- Snapshot application retains the existing sorting, selection and per-task view-state path. Stop completion and deletion checks wait for the requested snapshot without blocking input. Startup draws the existing loading state. Codex rollout/JSONL readers and transcript-panel internals are unchanged by this patch.

## Integration checks

`git archive 175c81c786768445a8421a00f29c24823abf0855` plus `delivery-code.patch`, independent `CARGO_TARGET_DIR`, host process/filesystem access. All 589 non-owned files match the archive blobs; all 13 owned source files match the delivered patch. No uncommitted peer changes are included. Formatting and already-committed peer work distinguish this integration source from the isolated performance comparison above.

| Check | Result |
|---|---|
| `cargo nextest run` | 1832 passed, 144 skipped, 0 failed |
| `cargo clippy --all-targets` | exit 0 |
| `cargo build --release` | exit 0 |

`checks.json` records commands, source/patch/binary checksums, timings and log checksums. `test-evidence.json` records the real-fixture red/green regressions for blocked input/exit, targeted rows, child paths, history pagination, stop state and liveness deadlines.
