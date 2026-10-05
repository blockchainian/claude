---
name: ios-debugger-agent
description: Build, run, and debug iOS apps on Simulator with XcodeBuildMCP. Use when launching an app, inspecting simulator UI or logs, or diagnosing runtime behavior.
---

# iOS Debugger Agent

## Overview
Use XcodeBuildMCP to build and run the current project scheme on a booted iOS simulator, interact with the UI, and read captured logs. Prefer the MCP tools for simulator control, logs, and view inspection.

Tool names below are unprefixed. Discover the tools exposed by the `xcodebuildmcp`
server through the current host's tool inventory or tool search, then call their
actual registered names. Claude Code and Codex use different namespace prefixes;
do not construct one. If a required tool is unavailable, report it before proceeding.

## Core Workflow
Follow this sequence unless the user asks for a narrower action.

### 1) Select and claim one simulator
- Call `list_sims` and choose a definite simulator UUID; if several are booted, select one explicitly.
- The coordinator assigns a different UUID to each parallel run. Never use `booted` as a target.
- Before booting, building, launching or interacting, claim that UUID with the existing sibling helper:

In each shell call, reassign `SKILL_DIR`, `UDID`, and `RUN_ID` to the resolved
values; shell variables do not persist between calls.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
UDID="<chosen-simulator-uuid>"
RUN_ID="$(uuidgen)"
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID"
```

- Exit 3 means another run owns the target: wait for its release or choose another UUID. Do not steal a live run.
- Keep this claim through the whole build/debug flow. Nested screenshot or profiling work uses the same UUID and run id; it must not release the outer run's claim.
- If the chosen simulator is shut down, call `boot_sim` with its explicit `simulatorId`.

### 2) Pass the target in every call
- This plugin sets `XCODEBUILDMCP_DISABLE_SESSION_DEFAULTS=true`, exposing per-call target parameters.
- Check the host's actual tool schema. If target tools omit `simulatorId`, report the stale server configuration and reconnect before parallel work.
- Never call `session_set_defaults` or switch an active defaults profile: those settings are shared by all agents using the server.
- Every simulator/UI call takes this run's explicit `simulatorId`. Every build or app-path lookup also supplies the current `projectPath` or `workspacePath`, `scheme`, and relevant configuration.
- Use a run-specific `derivedDataPath` for builds. Launch/stop calls also pass the exact `bundleId`; do not inherit another run's app target.
- Check the target UUID returned by each operation; a mismatch stops the flow.

### 3) Build + run (when requested)
- Call `build_run_sim` with `simulatorId`, the project/workspace, scheme, configuration and run-specific `derivedDataPath`.
- **If the build fails**, check the error output and retry (optionally with `preferXcodebuild: true` on that call) or escalate to the user before attempting any UI interaction.
- **After a successful build**, verify the app launched by calling `snapshot_ui` or `screenshot` before proceeding to UI interaction.
- If the app is already built and only launch is requested, use `launch_app_sim`.
- If bundle id is unknown:
  1) `get_sim_app_path`
  2) `get_app_bundle_id`

## UI Interaction & Debugging
Pass this run's explicit `simulatorId` on every UI action and snapshot.
UI actions target an `elementRef` from the latest snapshot, not coordinates or labels.

- **Snapshot**: `snapshot_ui` before tapping or typing. It returns `elementRef` targets and lists which actions each one supports.
- **Tap**: `tap` with an `elementRef` that lists `tap` in its snapshot targets.
- **Type**: `type_text` with the field's `elementRef`; set `replaceExisting` to overwrite contents.
- **Batch**: `batch` for several taps on one screen that need no assertion between them.
- **Gestures**: `gesture` with a `preset` for scrolls and edge swipes.
- **Screenshot**: `screenshot` for visual confirmation.

Re-snapshot after navigation, scrolling, or a sheet change — refs from a stale snapshot will not resolve.

## Logs & Console Output
`build_run_sim` and `launch_app_sim` capture runtime logs automatically and return the log file path in their response. Read that file directly and summarize the important lines; there is no separate start/stop log-capture tool.

## Cleanup

After the app interaction and any nested capture/profiling processes finish, release this
run's claim on success or failure. Preserve diagnostics first; never release while its
background process is still driving the simulator. Only the outer run releases a shared claim.

```sh
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID" --release
```

## Reporting
- Say which simulator and scheme were used.
- Summarize build result, launch result, and any UI steps taken.
- Quote the relevant log lines rather than the whole capture.
