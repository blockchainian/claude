---
name: ios-memgraph-leaks
description: Capture and inspect iOS leaks and memgraphs. Use when debugging leaked objects, retain cycles, memory growth, or before/after leak evidence.
---

# iOS memgraph leaks

This skill proves iOS leaks from a live simulator process or from an existing `.memgraph`. When the
task also needs a simulator build, install, launch, UI driving, logs or screenshots, pair it with
`../ios-debugger-agent/SKILL.md`.

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md`, never to the app repo's `pwd`:
installed plugins usually live outside the app being debugged. Shell variables do not persist
between calls, so reassign `SKILL_DIR`, `UDID` and `RUN_ID` to the same resolved values in every
shell call.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `MOBILE_DATA_DIR` | Scratch root; captures go under `tmp/ios-memgraph-leaks/`; default `~/.local/share/mobile` | Optional | Shell environment |

## Simulator ownership

Live simulator work needs an explicit simulator UUID, claimed before any build, launch or capture
and kept, with the same `RUN_ID`, for the whole run. Analyzing an existing memgraph needs no claim.

```sh
UDID="<chosen-simulator-uuid>"
RUN_ID="$(uuidgen)"
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID"
```

The coordinator gives parallel runs different UUIDs. Exit 3 means another run owns the simulator:
wait for it to release or choose another UUID, and never steal a live run. Inside an existing
debugger or screenshot run, reuse its claim and run id; only that outer run releases it.

Every XcodeBuildMCP call passes `simulatorId` explicitly. Build and app-path calls also pass
`projectPath` or `workspacePath`, `scheme`, the configuration and a run-specific `derivedDataPath`;
launch and stop calls pass `bundleId`. Do not change shared session defaults or active profiles.
Check that the exposed schema accepts `simulatorId` and that every response targets the chosen
UUID. On a mismatch, stop, or reconnect a server that still targets only shared defaults.

## Workflow

Build and launch the app, then drive the exact flow that should release the objects. Capture a
memgraph from the running process, summarize its leaks, and for each app-owned leaked type inspect
ownership with `leaks --traceTree=<address> <file.memgraph>` and the grouped leak evidence. Make the
smallest root-cause patch, then recapture the same flow, on the same simulator when possible, and
report the proof.

A smaller memgraph alone never proves a fix. A credible fix explains the ownership path that kept
the object alive and shows that the same path or type disappears after the patch.

## Capture

Capture from the simulator already used for the reproduction, with its UDID and the app's bundle
identifier:

```sh
"$SKILL_DIR/scripts/capture_sim_memgraph.sh" \
  --udid "$UDID" \
  --bundle-id "<app.bundle.identifier>"
```

Without `--out-dir`, each capture goes to a fresh run directory under the scratch root. A folder
passed with `--out-dir` should be run-specific or chosen by the user, never under `SKILL_DIR`. The
script prints the paths of the memgraph, the raw `leaks` output and a metadata file.

If it cannot find the process, confirm the bundle identifier and list the running labels with
`xcrun simctl spawn "$UDID" launchctl list`.

## Summarize

```sh
"$SKILL_DIR/scripts/summarize_memgraph_leaks.py" \
  /path/to/app.memgraph \
  --trace-limit 5 \
  --out /path/to/leak-summary.md
```

Keep `--trace-limit` small: trace trees are useful root-cause evidence, but large memgraphs make
them noisy. A trace tree that says `Found 0 roots referencing` marks an unreachable or self-retained
leak candidate; use the summary's grouped leak tree, or `leaks --groupByType <file.memgraph>`, to
find the retained fields and the payload chain.

## Root cause rules

- Identify the first app-owned leaked type in the leak output or trace.
- Determine its intended lifetime: process, session, account, view, request or task.
- Treat lazy or deferred allocation as a scope reduction, not a leak fix, unless the original eager
  allocation itself violated the intended lifetime.
- Prove a retain-cycle claim with either a `traceTree` ownership path or an isolated reproduction.
- For an unreachable or self-cycle leak, `traceTree` may show no root path; find the self-retaining
  edge with `leaks --groupByType` and by verifying the source.
- Never claim success because the total leak count went down; prove the specific type or path
  disappeared.
- Separate real root-cause branches from candidate and noise branches.
- Prefer deleting the retaining edge over adding broad cleanup code.

## Report

The leak report gives:

- the exact flow, the simulator and the app build
- the memgraph and summary paths
- the app-owned leaked types and their counts
- at least one ownership path, or grouped leak tree evidence when the object is unreachable from
  roots
- the smallest proposed or applied fix to the retaining edge
- before/after leak counts, the root types that disappeared and the leaks that remain, when a fix
  was made
- the test and build results

If the memgraph shows only framework or runtime noise, say so and recommend a narrower next capture
rather than inventing an app leak.

## Cleanup

Once capture and simulator driving have finished, keep the diagnostics, then release this run's own
claim, on success or failure. A claim reused from an outer run stays for that run to release.

```sh
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID" --release
```
