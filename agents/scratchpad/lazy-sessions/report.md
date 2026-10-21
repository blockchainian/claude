# Session discovery

## Selection

- Codex selects recent inactive IDs in SQLite with `ORDER BY` and `LIMIT`. Every child consumes a history slot. One extra ID determines `has_more`; only the requested IDs are decoded and enriched.
- Live Codex writers and live Claude registry entries seed the tree. Hub expands only those seeds and their descendants, with provider-qualified visited/selected sets.
- Native Codex children are queried through `thread_spawn_edges` by parent ID. Manager children come from the specified Claude parent's state plus their explicit thread registries. Recent rows recover ownership through only their own registry. Detached ownership survives.
- Claude indexes transcript file metadata, includes subagents in the same recent history budget, and reads summaries only for selected rows. Live-parent children remain available independently of pagination; overlap consumes a history slot and is deduplicated.
- Removed both orphan-pruning passes. A missing parent now clears only that row's parent link; the row and its loaded descendants remain visible.
- Inactive retains live ancestors with their real status and time. Rollout question reading and sending-answer behavior are unchanged.

## Red evidence

- 1600 old SQLite rows outside the requested page: eager discovery failed with `InvalidColumnType(22, "model", Blob)`.
- Recent native child with an older absent parent: eager discovery omitted the child.
- Missing-parent merge: returned zero rows instead of two.
- Unrelated corrupt manager registry: global lookup failed on the unrelated JSON.
- Thirty historical Claude children: loaded 31 rows instead of the ten-row budget.
- Latest guardian row: consumed a history slot, returning nine visible rows; SQL now excludes internal review rows before limiting.

## Regression coverage

Captured upstream SQLite schema/rows, Claude transcript/registry and manager attached/detached payloads are instantiated as real temporary files. The integration registry uses the running test process and its real libproc start time. Tests check exact selected membership, recursive native descendants, detached ownership, inactive-parent nonexpansion, orphan nonexpansion/flattening, reverse provider order, paging overlap, and the Inactive tree's actual status and time. No provider mocks were added.

Existing Inactive parent snapshot remains unchanged. UI rendering already supports the requested ancestor behavior.

## Checks

- Full `cargo nextest run`: 1790 passed, 0 failed, 141 intentionally skipped; 76.838 s test runtime (87.496 s including build).
- `cargo clippy --all-targets`: exit 0; 5.145 s. Existing unused-import/dead-code warnings remain.
- Existing inactive-parent snapshot passes unchanged; no snapshot files moved.
- Real tmux probes: finished Claude descendants, live ancestor Working/Ready with `2h ago` on Inactive, and stable finished `16s` elapsed time all passed. The obsolete ancestor-hiding assertion was first confirmed failing, then updated to the required status/time behavior. All owned probe servers ran `kill-server`.
- `cargo build --release`: exit 0; 9.088 s.
- Final `cargo clippy --all-targets` after the terminal-probe edit: exit 0.

## Real-home performance

Same owned 200×50 PTY scripts as `startup-slow/report.md`, real provider homes, five idle `ps` readings two seconds apart. Measurements use the installed pre-change binary and the release build. See `perf/report.md` and the ignored raw JSON/ANSI captures.

| Metric | cbea152a report | Current installed before | Release after |
| --- | ---: | ---: | ---: |
| First content | 1.532 s | 1.625 s | 1.564 s |
| Complete first frame | 1.651 s | 1.744 s | 1.682 s |
| Standalone first content | 1.852 s | 1.558 s | 0.584 s |
| Key Down task change | 59.555 ms | 27.798 ms | 78.328 ms |
| Click task | 0.608 ms | 0.529 ms | 0.510 ms |
| Click event | 105.149 ms | 453.361 ms | 105.153 ms |
| Idle ps CPU samples | 75.0/4.6/0/0/1.6% | 0.2/90.3/70.8/5.1/99.9% | 0.2/67.3/0/0/25.8% |

Full-probe startup is similar; this run's keyboard event is slower. Standalone startup is lower, and idle samples still contain refresh bursts. The five samples do not measure time-averaged CPU. Live sessions and concurrent activity differ from the earlier cbea152a run. Both performance probes passed and killed/reaped their own children; no tmux server was created by those PTY probes.
