---
name: ios-memgraph-leaks
description: Capture and inspect iOS leaks and memgraphs. Use when debugging leaked objects, retain cycles, memory growth, or before/after leak evidence.
---

# iOS memgraph leaks

Use this skill to prove iOS leaks from a live simulator process or an existing `.memgraph`. Pair it with `../ios-debugger-agent/SKILL.md` when the task also needs simulator build, install, launch, UI driving, logs, or screenshots.

## Simulator ownership

For live simulator work, select an explicit UUID and claim it before build, launch or capture.
Use this skill's actual loaded directory, and keep the same `UDID` and `RUN_ID` throughout:

In each shell call, reassign `SKILL_DIR`, `UDID`, and `RUN_ID` to the resolved
values; shell variables do not persist between calls.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
UDID="<chosen-simulator-uuid>"
RUN_ID="$(uuidgen)"
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID"
```

The coordinator assigns different UUIDs to parallel runs. Exit 3 means the chosen target is
owned: wait for release or choose another UUID; do not steal a live run. When called inside an
existing debugger/screenshot run, reuse its claim and run id; only the outer run releases it.
Analyzing existing artifacts needs no simulator claim.

XcodeBuildMCP calls must pass `simulatorId` explicitly; build and app-path calls also pass
`projectPath` or `workspacePath`, `scheme`, configuration and a run-specific `derivedDataPath`.
Launch/stop calls pass `bundleId`. Do not change shared session defaults or active profiles.
Check that the exposed schema accepts `simulatorId` and each response targets the chosen UUID;
stop on a mismatch or reconnect a server that still exposes only shared-default targeting.

## Core Workflow

1. Build, launch, and drive the exact flow that should release objects.
2. Capture a memgraph from the running simulator process with `scripts/capture_sim_memgraph.sh`.
3. Summarize leaks with `scripts/summarize_memgraph_leaks.py`.
4. For each app-owned leaked type, inspect ownership with `leaks --traceTree=<address> <file.memgraph>` and grouped leak evidence.
5. Make the smallest root-cause patch, then recapture the same flow on the same simulator when possible.
6. Report proof: before/after leak counts, disappeared root types, remaining leaks, memgraph paths, and test/build results.

Do not claim a leak fix from a smaller memgraph alone. A credible fix explains the ownership path that kept the object alive and shows that the same path or type disappears after the patch.

## Capture

Prefer capturing from the simulator already used for the reproduction. Resolve the simulator UDID and app bundle identifier, then capture the running app:

```bash
SKILL_DIR="<absolute path to this loaded skill folder>"
SIM="$UDID"
BUNDLE_ID="<app.bundle.identifier>"
mkdir -p "${MOBILE_DATA_DIR:-$HOME/.local/share/mobile}/tmp/ios-memgraph-leaks"
MEMGRAPH_DIR="$(mktemp -d "${MOBILE_DATA_DIR:-$HOME/.local/share/mobile}/tmp/ios-memgraph-leaks/run.XXXXXX")"

"$SKILL_DIR/scripts/capture_sim_memgraph.sh" \
  --udid "$SIM" \
  --bundle-id "$BUNDLE_ID" \
  --out-dir "$MEMGRAPH_DIR"
```

Do not derive `SKILL_DIR` from the target app repo's `pwd`; installed plugins usually live outside the app being debugged. Store captures in a run-specific temp or user-chosen folder, not under `SKILL_DIR`.

If the process cannot be found, confirm the bundle identifier and use `xcrun simctl spawn "$SIM" launchctl list` to inspect running labels.

## Summarize

Summarize an existing memgraph:

```bash
"$SKILL_DIR/scripts/summarize_memgraph_leaks.py" \
  /path/to/app.memgraph \
  --trace-limit 5 \
  --out /path/to/leak-summary.md
```

Use `--trace-limit` sparingly. Trace trees are useful root-cause evidence, but large memgraphs can produce noisy output. If a trace tree says `Found 0 roots referencing`, treat it as an unreachable/self-retained leak candidate and use the summary's grouped leak tree or `leaks --groupByType <file.memgraph>` to identify the retained fields and payload chain.

## Root Cause Rules

- Identify the first app-owned leaked type in the leak output or trace.
- Determine the intended lifetime: process, session, account, view, request, or task.
- Treat lazy or deferred allocation as a scope reduction, not a leak fix, unless the original eager allocation itself violated the intended lifetime.
- Prove retain-cycle claims with either a `traceTree` ownership path or an isolated reproduction.
- For unreachable/self-cycle leaks, `traceTree` may have no root path; use `leaks --groupByType` plus source verification to find the self-retaining edge.
- Do not claim success just because total leak count went down; prove the specific type or path disappeared.
- Separate real root-cause branches from candidate/noise branches.
- Prefer deleting the retaining edge over adding broad cleanup code.

## Report

A useful leak report includes:

- the exact flow and simulator/app build
- the memgraph and summary paths
- app-owned leaked types and counts
- at least one ownership path, or grouped leak tree evidence when the object is unreachable from roots
- the smallest proposed or applied retaining-edge fix
- before/after evidence when a fix was made

If the memgraph shows only framework/runtime noise, say that and recommend the next narrower capture rather than inventing an app leak.

## Cleanup

After capture and simulator-driving processes finish, release this run's own claim on success
or failure; an outer caller retains and releases its shared claim. Preserve diagnostics first.

```sh
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID" --release
```
