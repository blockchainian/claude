---
name: ios-debugger-agent
description: Build, run, and debug iOS apps on Simulator with XcodeBuildMCP. Use when launching an app, inspecting simulator UI or logs, or diagnosing runtime behavior.
---

# iOS debugger agent

This skill builds and runs the current project's scheme on one iOS simulator with the
XcodeBuildMCP tools, drives its UI and reads the captured logs. Use those tools, not shell
commands, for simulator control, logs and view inspection. Tool names below are unprefixed: call
the `xcodebuildmcp` server's tools by the names the host registers, never a prefix you build. If a
tool you need is missing, report it before going on.

Follow the workflow below in order unless the user asks for a narrower action.

## Skill directory

The simulator claim uses the sibling `ios-take-screenshot` skill's helper, so keep the whole mobile
plugin installed. Shell variables do not persist between calls, so set `SKILL_DIR`, `UDID` and
`RUN_ID` to their resolved values in every shell call.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
UDID="<chosen-simulator-uuid>"
RUN_ID="$(uuidgen)"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `MOBILE_STATE_DIR` | Root for simulator claims, shared by every run of the same user; default `~/.local/state/mobile` | Optional | Shell environment |

## Claim one simulator

Call `list_sims` and choose one definite simulator UUID; if several are booted, pick one explicitly.
Never target `booted`. When runs go in parallel, the coordinator gives each its own UUID. Claim that
UUID before booting, building, launching or interacting:

```sh
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID"
```

Exit 3 means another run holds the simulator: wait for it to release, or choose another UUID, and
never steal a live run's claim. Hold the claim for the whole build and debug flow. Nested screenshot
or profiling work reuses the same UUID and run id and must not release the outer run's claim. If the
chosen simulator is shut down, boot it with `boot_sim` and its explicit `simulatorId`.

## Pass the target in every call

The mobile plugin sets `XCODEBUILDMCP_DISABLE_SESSION_DEFAULTS=true`, which exposes target
parameters on each call. Check the host's actual tool schema: if the target tools have no
`simulatorId` parameter, the server configuration is stale, so report it and reconnect before any
parallel work. Never call `session_set_defaults` or switch the active defaults profile, because every
agent using the server shares those settings.

- Every simulator and UI call passes this run's `simulatorId`.
- Every build or app-path lookup passes the current `projectPath` or `workspacePath`, the `scheme`
  and the relevant configuration.
- Every build uses a `derivedDataPath` of its own for this run.
- Every launch or stop passes the exact `bundleId`, never another run's app.

Check the simulator UUID each operation returns; a mismatch stops the flow.

## Build and run

When the user asks for a build, call `build_run_sim` with the `simulatorId`, the project or
workspace, the scheme, the configuration and the run's `derivedDataPath`. If the build fails, read
the error output, then retry, optionally with `preferXcodebuild: true` on that call, or escalate to
the user; do not touch the UI until a build succeeds. After a successful build, confirm the app
launched with `snapshot_ui` or `screenshot` before any UI interaction.

If the app is already built and the user only wants it launched, call `launch_app_sim`. If the bundle
id is unknown, get it with `get_sim_app_path` and then `get_app_bundle_id`.

## Drive the UI

UI actions take an `elementRef` from the latest snapshot, never coordinates or labels.

- `snapshot_ui` before tapping or typing returns the `elementRef` targets and the actions each one
  supports.
- `tap` takes an `elementRef` whose snapshot entry lists `tap`.
- `type_text` takes the field's `elementRef`; set `replaceExisting` to overwrite what it holds.
- `batch` runs several taps on one screen that need no check between them.
- `gesture` with a `preset` scrolls or swipes from an edge.
- `screenshot` gives visual confirmation.

Take a new snapshot after navigation, scrolling or a sheet change, because refs from an older
snapshot no longer resolve.

## Read the logs

`build_run_sim` and `launch_app_sim` capture the runtime logs and return the log file's path; there
is no separate tool to start or stop a capture. Read that file and summarize the lines that matter.

## Release the simulator

Once the app interaction and any nested capture or profiling have finished, release the claim,
whether the run succeeded or failed. Save diagnostics first, and never release while a background
process of this run is still driving the simulator. Only the outer run releases a shared claim.

```sh
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID" --release
```

## Report

Say which simulator and scheme you used, summarize the build result, the launch result and the UI
steps taken, and quote the relevant log lines rather than the whole capture.
