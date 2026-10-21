# Codex rollout refresh fix

## Changes

- `Tasks` caches each file generation by device, inode, length and mtime. Unchanged files reuse the complete `Summary`; append reads resume at the last complete line. Replacement, truncation, same-length rewrite and boundary overwrite reset the generation. Valid unterminated summary records remain visible; plans still wait for a newline. Errors invalidate the cache before retry.
- Appended JSON is decoded once for summary, activity, plan and first-prompt selection. Retained records omit raw response bodies and injected context fields the consumers never read. Metadata and `history_start` are cached; attachment existence is still checked against the filesystem, and prompt/completion pairing preserves physical line adjacency.
- Backward paging moves retained events and borrows JSON Values. Only newly loaded records and rows affected by newly discovered completions enter event conversion. Pairing context remains borrowed; result text is assembled only for selected calls. Context scans are batched with geometric growth when more history is needed, and dependencies update only for incoming records. Merging, turn markers and `previous_end` are restored after combining entries.
- Forward JSONL buffers use `min(unread bytes, 8 MiB)`; an EOF read allocates no chunk. A captured 672-byte append uses 672 bytes instead of 8,388,608.
- `background()` keeps its short-lived daemon query. Terminal process liveness lives in daemon memory and can change without a rollout write. The inspected daemon implementation automatically subscribes initialized connections to newly created threads; keeping this client connected would change thread residency and incoming interactive requests.

## Measurements

Both release binaries use `git archive 9d382e69`; the after copy adds only this patch. Each binary was rebuilt after clearing this package's release artifacts and retained separately. Binary SHA-256 values are in `results.json`.

Real HOME, no Codex/Claude home overrides, 200×50 PTY with the original OSC11/DA1 replies. Three startup runs alternate before/after. Each idle run takes ps snapshots every 0.5 seconds from first content + 4 seconds through + 26 seconds; CPU is the change in process CPU time divided by elapsed time. A 10-second, 1 ms `sample` starts at first content + 6 seconds. `NO_COLOR=1` for both binaries. Live sessions continue writing during the runs.

| Metric | Before, same base | After |
|---|---:|---:|
| First content, median of 3 | 1.774548 s | 1.214923 s |
| Idle process CPU, one core = 100% | 14.9240% | 2.4993% |
| ps peak CPU | 95.5% | 50.7% |
| ps observations | 43 | 44 |
| Main-thread sample observations | 8742 | 8773 |
| Inclusive `Tasks::summary` / cache refresh samples | 1078 | 11 |
| Inclusive app/session refresh samples | 1182 | 68 |
| `clone_subtree` samples, entire main thread | 169 | 3 |

Startup seconds: before 2.186922, 1.774548, 1.587981; after 1.831233, 1.214923, 1.016670.

The earlier [summary](../summary.md) reported first content 1.85 s, 0.4–0.6 s CPU per refresh, and rollout summary at 75–87% of refresh CPU. In the same-base comparison above, summary is 91.20% of refresh samples before and 16.18% after; its absolute sample count drops 98.98%. It is no longer the main idle hotspot. The measured idle CPU mean drops 83.25%.

Caching retains the fields needed for future cross-record pairing. `sample` reports physical footprint 121.2M before and 211.3M after; the cache trades memory for avoiding repeated parsing. Raw response bodies and labelled injected context are excluded from retained records.

Reproduce a run with `python3 probe.py <release-binary> <label> --idle`, then `python3 metrics.py`. Raw local evidence: `delivery-*-startup-*.json`, `delivery-*-idle.json`, `delivery-*-idle.sample.txt`; small extracted results are checked into `results.json`. Timing runs use PTYs and create no tmux server.

## Regression and gates

- The unchanged-file test first failed with 1,046 parsed bytes instead of 523. Incremental real-fixture prefixes now equal the full parser, including plans, questions, cache expiry, activity and source identities; unchanged refreshes add zero parsed bytes.
- Backward paging first attempted 2,057 event conversions for 640 captured records. It now converts each new record once, with completion repairs tested across pages; expanding pages preserve the full-parser events. A record larger than 64 KiB also has an explicit progress regression.
- Real-file regressions cover replacement, truncation, same-length rewrite, a metadata line completed after an empty read, valid unterminated records, error retries, attachment deletion and invalid physical lines between prompt/completion. Compact-record retention has its own red/green test.
- `cargo nextest run --test-threads 4 --no-fail-fast`: **1815 passed, 0 failed, 144 skipped**. One existing UI navigation test was slow. An earlier high-concurrency run hit the second-sensitive `2m 0s` border assertion; that test passed against the unmodified base, and the final full run passed without changing it.
- `cargo clippy --all-targets`: passed; existing warnings remain outside this change.
- `cargo build --release`: passed.
- Three existing tmux probes passed: Codex agent-message bodies, collaboration navigation and memory-citation details. Their guard executes `tmux -L <name> kill-server` on completion.

All gates ran in the archived copy with a private `CARGO_TARGET_DIR`. No installation was performed.
