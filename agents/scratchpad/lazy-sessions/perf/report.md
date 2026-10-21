# Real-home performance

## Method

Copied `startup-slow/perf.py` and `probe.py`. Real HOME, no Claude/Codex/manager directory overrides, owned 200×50 PTY, OSC11/DA1 responses, monotonic exec-to-content/frame and event-to-render timings. The interaction probe retains the original task/event screen assertions and five `ps %CPU,TIME` readings two seconds apart. The interaction probe has an overall 120-second deadline. Each probe kills and reaps only its child. No tmux server is created, so there is no tmux server to kill. Raw session screens, logs, JSON and copied binaries remain ignored.

`baseline-agents` is a copy of the installed binary before source changes. CPU sampling requires the host process capability. The sandbox-only attempt could render but failed on `ps`; the successful baseline below ran with that capability.

## Comparison

| Metric | Prior cbea152a report | Installed baseline, current run | Release after |
| --- | ---: | ---: | ---: |
| First content | 1.532 s | 1.625 s | 1.564 s |
| Full first frame | 1.651 s | 1.744 s | 1.682 s |
| Standalone first content | 1.852 s | 1.558 s | 0.584 s |
| Key Down task change | 59.555 ms | 27.798 ms | 78.328 ms |
| Click task | 0.608 ms | 0.529 ms | 0.510 ms |
| Click event | 105.149 ms | 453.361 ms | 105.153 ms |
| Idle ps %CPU, five samples | 75.0, 4.6, 0.0, 0.0, 1.6 | 0.2, 90.3, 70.8, 5.1, 99.9 | 0.2, 67.3, 0.0, 0.0, 25.8 |

The prior column is copied from `startup-slow/report.md`, not remeasured. Live sessions and concurrent workload can change between runs. These five instantaneous CPU readings measure refresh bursts rather than a time-averaged idle CPU value.

Raw evidence: `baseline.perf.json`, `baseline-standalone.json`, matching ANSI/screens, and `baseline.log`. Both probes passed and their children were reaped.

The release interaction and standalone probes both passed. The release binary was read from `target/release/agents` without rebuilding. Startup varied between the interaction and standalone runs; the interaction first frame was close to baseline, while standalone first content was lower. Key Down was slower in this sample (78.328 ms versus 27.798 ms); the samples do not establish an across-the-board latency improvement. Idle CPU still bursts during refresh, with two release samples at 0.0%.

After evidence: `after.perf.json`, `after-standalone.json`, matching ANSI/screens, and logs. Both release probe children were killed and reaped. No tmux server was created.
