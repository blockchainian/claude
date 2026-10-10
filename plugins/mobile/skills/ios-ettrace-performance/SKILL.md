---
name: ios-ettrace-performance
description: Capture and interpret iOS Simulator ETTrace profiles. Use when profiling launch or runtime latency, comparing traces, or finding CPU-heavy stacks.
---

# iOS ETTrace performance

This skill captures one focused, symbolicated ETTrace profile from an iOS simulator app and reads
it. When the task also needs a simulator build, install, launch, UI driving, logs or screenshots,
use it together with `../ios-debugger-agent/SKILL.md`.

Before anything else, pick one focused flow and write down where it starts and stops, then build
the exact simulator app you will install and profile; the sections below follow the rest of the
session in order. Avoid broad "use the app for a while" captures: one trace covers one
user-visible flow.

## Skill directory

Shell variables do not persist between calls, so in every shell call reassign `SKILL_DIR` to the
absolute directory of this loaded skill, and `UDID` and `RUN_ID` to the values you chose for this
session; keep the same `UDID` and `RUN_ID` throughout.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `MOBILE_DATA_DIR` | Root for scratch run folders, under `tmp/ios-ettrace-performance/`; default `~/.local/share/mobile` | Optional | Shell environment |
| `MOBILE_STATE_DIR` | Root for simulator claims; default `~/.local/state/mobile` | Optional | Shell environment |
| `RUN_DIR` | Reuse an existing writable run folder instead of creating one | Optional | Shell environment |
| `ETTRACE_TAG` | ETTrace source tag for the app-side framework build; default `v1.1.0`; set it to match the installed runner when Homebrew updates | Optional | Shell environment |

## Setup

Give each profiling session its own writable run folder:

```bash
if [ -z "${RUN_DIR:-}" ]; then
  mkdir -p "${MOBILE_DATA_DIR:-$HOME/.local/share/mobile}/tmp/ios-ettrace-performance"
  RUN_DIR="$(mktemp -d "${MOBILE_DATA_DIR:-$HOME/.local/share/mobile}/tmp/ios-ettrace-performance/run.XXXXXX")"
fi
mkdir -p "$RUN_DIR"
```

Install the host-side runner if `ettrace` is not already available:

```bash
brew install emergetools/homebrew-tap/ettrace
```

The app must also link an `ETTrace.xcframework` built for the iOS Simulator architecture. This
workflow is validated for ETTrace v1.1.0 processed `output_<thread>.json` files with top-level
`nodes`.

## Simulator ownership

For live simulator work, pick an explicit simulator UUID and claim it before you build, launch or
capture. Analyzing existing artifacts needs no claim.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
UDID="<chosen-simulator-uuid>"
RUN_ID="$(uuidgen)"
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID"
```

The coordinator gives parallel runs different UUIDs. Exit 3 means another run owns the target: wait
for it to release or choose another UUID, and never steal a live run. When this skill runs inside an
existing debugger or screenshot run, reuse that run's claim and run id; only the outer run releases
it.

Every XcodeBuildMCP call passes `simulatorId` explicitly. Build and app-path calls also pass
`projectPath` or `workspacePath`, `scheme`, the configuration and a run-specific `derivedDataPath`;
launch and stop calls pass `bundleId`. Do not change shared session defaults or active profiles.
Check that the exposed schema accepts `simulatorId` and that each response targets the chosen UUID.
On a mismatch, stop, or reconnect a server that still offers only shared-default targeting.

## Link ETTrace into the app

Wire ETTrace into the exact app target you are profiling, as a clearly temporary patch. Reuse a
simulator-compatible `ETTrace.xcframework` if the repo already vendors one; otherwise build a
simulator-only copy into `RUN_DIR` from the upstream package:

```bash
ETTRACE_TAG="${ETTRACE_TAG:-v1.1.0}" # Override to match the installed runner when Homebrew updates.
ETTRACE_SRC="$RUN_DIR/ETTrace-src"
if [ ! -d "$ETTRACE_SRC" ]; then
  git clone --depth 1 --branch "$ETTRACE_TAG" https://github.com/EmergeTools/ETTrace "$ETTRACE_SRC"
fi

rm -rf "$RUN_DIR/ETTrace-iphonesimulator.xcarchive" "$RUN_DIR/ETTrace.xcframework"
pushd "$ETTRACE_SRC" >/dev/null
xcodebuild archive \
  -scheme ETTrace \
  -archivePath "$RUN_DIR/ETTrace-iphonesimulator.xcarchive" \
  -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  BUILD_LIBRARY_FOR_DISTRIBUTION=YES \
  INSTALL_PATH='Library/Frameworks' \
  SKIP_INSTALL=NO \
  CLANG_CXX_LANGUAGE_STANDARD=c++17

xcodebuild -create-xcframework \
  -framework "$RUN_DIR/ETTrace-iphonesimulator.xcarchive/Products/Library/Frameworks/ETTrace.framework" \
  -output "$RUN_DIR/ETTrace.xcframework"
popd >/dev/null
```

Link the framework directly into the app target, not only into tests, resources, data files or a
nested launcher target. In a Bazel app, a temporary import usually looks like this:

```python
load("@rules_apple//apple:apple.bzl", "apple_dynamic_xcframework_import")

package(default_visibility = ["//visibility:public"])

apple_dynamic_xcframework_import(
    name = "ETTrace",
    xcframework_imports = glob(["ETTrace.xcframework/**"]),
)
```

In an Xcode project, add the simulator `ETTrace.xcframework` to the app target's **Link Binary With
Libraries** and **Embed Frameworks** phases for the debug simulator build you are profiling.

After launch, confirm the logs print `Starting ETTrace`. Simulator mode listens on a fixed localhost
port, and a different UUID does not isolate it, so profile only one ETTrace-instrumented simulator
app on the host at a time: serialize captures and make sure only the selected app connects.

## Collect dSYMs

Never draw conclusions from an unsymbolicated flamegraph. Before every capture, and after the final
build that produced the installed app, collect the app dSYM and every embedded first-party dynamic
framework dSYM:

```bash
SKILL_DIR="<absolute path to this loaded skill folder>"
APP="<path-to-built-simulator-App.app>"
DSYMS="$RUN_DIR/dsyms"

"$SKILL_DIR/scripts/collect_ios_dsyms.sh" \
  --app "$APP" \
  --out-dir "$DSYMS" \
  --search-root "$(dirname "$APP")" \
  --search-root "$PWD" \
  --extra-dsym "$RUN_DIR/ETTrace-iphonesimulator.xcarchive/dSYMs/ETTrace.framework.dSYM"
```

Add `--require-framework <FrameworkName>` for each app-owned dynamic framework that must
symbolicate, and use `--require-all-frameworks` only when every embedded framework is app-owned or
expected to have symbols. If the script reports a missing required app or framework dSYM, rebuild
the exact simulator app with dSYM generation before tracing, or add the build output directory that
holds those dSYMs as another `--search-root`.

When the report looks suspicious, verify the important UUIDs before tracing:

```bash
dwarfdump --uuid "$APP/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Info.plist")"
find "$DSYMS" -maxdepth 1 -type d -name '*.dSYM' -print -exec dwarfdump --uuid {} \;
```

After ETTrace exits, read its symbolication summary. Meaningful first-party "have library but no
symbol" lines mean the trace failed, unless they are tiny noise; unsymbolicated system-framework or
ETTrace internal buckets are usually fine.

## Capture

Run `ettrace` attached to a TTY so it can read its interactive prompts; without one it can exit
without a useful trace. Each capture first writes a start marker and clears old outputs. A launch
trace, for startup or first render only, uses `--launch`:

```bash
cd "$RUN_DIR"
CAPTURE_MARKER="$RUN_DIR/.ettrace-capture-start"
: > "$CAPTURE_MARKER"
find "$RUN_DIR" -maxdepth 1 \( -name 'output.json' -o -name 'output_*.json' \) -delete
ettrace --simulator --launch --verbose --dsyms "$DSYMS"
```

The first launch connection can force-quit the app; if prompted, relaunch it from the simulator home
screen rather than from Xcode. For a first-launch-after-install trace, temporarily set
`ETTraceRunAtStartup=YES` in the app's Info.plist, run `ettrace --simulator`, and launch from the
home screen.

A runtime flow trace drops `--launch`:

```bash
cd "$RUN_DIR"
CAPTURE_MARKER="$RUN_DIR/.ettrace-capture-start"
: > "$CAPTURE_MARKER"
find "$RUN_DIR" -maxdepth 1 \( -name 'output.json' -o -name 'output_*.json' \) -delete
ettrace --simulator --verbose --dsyms "$DSYMS"
```

Start from a stable screen, start ETTrace, perform exactly one focused flow, wait until the visible
work is done, then stop the runner. Start with the main thread, and add `--multi-thread` when you
need wider attribution.

## Preserve outputs

The next ETTrace run can overwrite the processed flamegraph files, so copy the fresh
`output_<thread-id>.json` files right after each run. This also writes a `summary.txt` from the
analyzer:

```bash
PRESERVED_DIR="$(mktemp -d "$RUN_DIR/run-$(date +%Y%m%d-%H%M%S).XXXXXX")"
: > "$PRESERVED_DIR/summary.txt"
if [ ! -e "$CAPTURE_MARKER" ]; then
  echo "error: capture marker missing; start a fresh ETTrace capture before preserving outputs" >&2
  exit 1
fi
find "$RUN_DIR" -maxdepth 1 -name 'output_*.json' -newer "$CAPTURE_MARKER" -print | while IFS= read -r json; do
  preserved="$PRESERVED_DIR/${json##*/}"
  cp "$json" "$preserved"
  {
    echo "## ${preserved##*/}"
    python3 "$SKILL_DIR/scripts/analyze_flamegraph_json.py" "$preserved"
  } >> "$PRESERVED_DIR/summary.txt"
done
if [ ! -s "$PRESERVED_DIR/summary.txt" ]; then
  echo "error: no fresh processed ETTrace output JSON found in $RUN_DIR" >&2
  exit 1
fi
```

Analyze only the processed `output_*.json` files from `RUN_DIR`. Ignore `output.json`, which is also
the name of ETTrace's viewer route, and raw `emerge-output/output.json` files, which are not
processed flamegraphs, unless you are debugging ETTrace itself. If the analyzer rejects the JSON
shape, capture again with the Homebrew runner and an app-side `ETTrace.xcframework` of the matching
tag instead of interpreting the rejected file.

## Read the profile

Start from `run-*/summary.txt`, and open the processed JSON directly only when you need more. The
report covers:

- the exact flow, app build, simulator model and runtime, and run count
- the processed flamegraph JSON paths
- the top active leaves and inclusive first-party stacks, with sample weights or percentages
- whether symbols were complete for app-owned binaries
- caveats such as first-run setup, simulator-only cost, network variance or a low sample count
- before/after deltas, only when the same flow was captured with comparable setup

## Cleanup

Remove the temporary ETTrace app wiring when profiling is done, unless the user asked to keep it,
and keep or discard the run artifacts as the task requires. Once ETTrace and every
simulator-driving process have finished, release this run's own claim, on success or failure; an
outer caller keeps and releases its shared claim itself.

```sh
node "$SKILL_DIR/../ios-take-screenshot/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID" --release
```
