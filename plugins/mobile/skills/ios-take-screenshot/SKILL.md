---
name: ios-take-screenshot
description: Capture one whole iOS app screen as a single stitched PNG, including everything below the fold. Use when asked to screenshot an app screen, capture a full page, or collect screens of another app for design research. Drives either a real iPhone connected over USB, through appium-mcp, or a booted simulator, through XcodeBuildMCP.
---

# iOS take screenshot

iOS screenshots return only the visible viewport, so this skill captures a whole screen as overlapping slices and stitches them into exactly one PNG per requested screen: open the app, reach the screen, capture slices, stitch, delete the slices.

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call that runs a script, using the installed path rather than the app repository's working directory or a host-specific plugin variable. If the loaded path is unavailable, stop and report it before running a script.

```bash
SKILL_DIR="/absolute/path/to/loaded/skill"
```

Variables do not survive between shell calls. Wherever this document writes `$SLICE_DIR`, `$OUT_ROOT`, `$UDID` or `$RUN_ID`, type out the absolute value in full, or put the whole capture loop in one command. Getting this wrong fails quietly: an `if [ -z "${SLICE_DIR:-}" ]` guard mints a new directory on every command, one slice in each, and the stitch succeeds on that single slice without complaint.

The scripts print their results on stdout; the one-line package note `uv` prints on a first run goes to stderr, so parse stdout alone.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `MOBILE_OUTPUT_DIR` | Output root; screenshots go under `ios-screenshots/<run>/`; default `~/Documents` | Optional | First line of the run-setup command |
| `MOBILE_DATA_DIR` | Data root; slices go under `tmp/ios-take-screenshot/` | Optional | Shell environment |
| `MOBILE_STATE_DIR` | State root; device claims go under `locks/`, shared by every session of the same user | Optional | Shell environment |
| `CAPABILITIES_CONFIG` | Path to a local `capabilities.json` the appium-mcp server reads instead of inline capabilities | Optional | appium-mcp server environment |
| `SCREENSHOTS_DIR` | Where `appium_screenshot` writes; otherwise the server's working directory | Optional | appium-mcp server environment |

## Setup

### Pick the target

Every step has a real-device path and a simulator path, which differ only in how the app is found, how the session is set up, and which calls scroll and capture. Stitching, output naming, settling, the endless-list rule, cleanup and reporting are the same on both.

| | Real device | Simulator |
|---|---|---|
| App lookup | `devicectl` | `simctl` |
| Drive layer | appium-mcp | XcodeBuildMCP |
| Capture | `appium_screenshot` | `xcrun simctl io` |

Use the device when the request names one, when the app is installed only on a phone, or when the point is how the app behaves on real hardware. Use the simulator when the app is already running there, when no phone is connected, or when the request is about layout and content. If the request does not say and both are available, ask.

Tool names here are unprefixed: find the actual registered names of the appium-mcp tools (such as `appium_screenshot`) and the XcodeBuildMCP tools (such as `swipe`) in the host's tool inventory or tool search, and never construct a prefix. If a required tool is unavailable, report it before going on.

### Set the run up once

Run this one command and read the three values from its output:

```bash
RUN_ID="$(date +%Y%m%d-%H%M%S)-$$"
SCRATCH="${MOBILE_DATA_DIR:-$HOME/.local/share/mobile}/tmp/ios-take-screenshot"
mkdir -p "$SCRATCH"
SLICE_DIR="$(mktemp -d "$SCRATCH/slices.XXXXXX")"
OUT_ROOT="${MOBILE_OUTPUT_DIR:-$HOME/Documents}/ios-screenshots/$RUN_ID"
mkdir -p "$SLICE_DIR" "$OUT_ROOT"
printf 'RUN_ID=%s\nSLICE_DIR=%s\nOUT_ROOT=%s\n' "$RUN_ID" "$SLICE_DIR" "$OUT_ROOT"
```

`MOBILE_OUTPUT_DIR` takes effect only when it is set in the shell that runs this command, so to use it make `MOBILE_OUTPUT_DIR=/some/dir` the first line of that same command. `RUN_ID` gives each run its own output subdirectory, so two sessions capturing the same screen cannot overwrite each other, and it is also the name under which this run claims its device. Each screen is saved as:

```
$OUT_ROOT/<app-slug>/<screen-slug>.png
```

A path given in the request always wins over this default; pass it to the stitcher's `--out`. Keep slices in `SLICE_DIR`, never under the skill folder.

### Claiming a device

Device use is exclusive per UDID. `claim-simulator.mjs` holds one simulator or phone for one `RUN_ID`, and `capture-slice.sh` refuses a simulator that is unclaimed or claimed by another run. Different simulators can run in parallel; several callers share one phone in turn, waiting and claiming again after the holder releases it, with no background queue. Only the agent assigned to a run claims and releases it, and a run capturing several screens keeps its claim until the last one, because releasing between screens invites another agent in mid-run. Nested screenshot or profiling work reuses the outer run's UDID and `RUN_ID`, and only the outer run releases the claim. Never expire a claim because its timestamp is old: profiling can take a long time.

### Real device

Before any phone automation, check that the plugin's `phone-session-gate` hook is active; in Codex, review and trust the loaded hook in `/hooks`. If the gate is not active or not trusted, stop phone automation and report it, because the written claim workflow does not replace the hook.

Discover the session values rather than asking the user for them or carrying them between sessions:

```bash
"$SKILL_DIR/scripts/discover-ios-setup.mjs"
```

It reports the connected devices, which provisioning profiles cover them, whether WebDriverAgent is installed, and, when everything is in place, a `suggestedCapabilities` object for `appium_session_management` (`action=create`). Exit 0 means ready; exit 1 lists what is missing. The UDID, team id and WebDriverAgent bundle id differ per machine and are not in the plugin config: pass them inline to `appium_session_management` (`action=create`), or point the server at a `capabilities.json` with `CAPABILITIES_CONFIG`.

Before preparing WebDriverAgent or creating the session, claim the phone so no other agent opens a session that ends yours:

```bash
"$SKILL_DIR/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID"
```

Exit 0 means it is yours. Exit 3 means another run holds it: wait and claim again, since there is no other phone to pick. Hold the claim across every screen of the flow.

The gate binds the claimed phone to the host session and agent that first prepares WebDriverAgent or creates a session, and it records that session's id in the claim. Give the whole phone task to that one agent and do not delegate phone operations while holding the claim; the gate denies operations by the main agent or a sibling when a subagent owns the task. Pass the explicit `sessionId` on every Appium phone operation, because shared active-session defaults are denied. Session listing and device discovery stay open to everyone. Do not detach an owned session. Deleting a session keeps the task owner, across failed creates and reconnects, until the claim is released; reclaiming with the same `RUN_ID` keeps ownership; and release is refused while a session or create is recorded.

If WebDriverAgent is not installed, do not build it by hand; use `appium_prepare_ios_real_device`:

1. Call it with no `provisioningProfileUuid` to list the available profiles.
2. Call it again with the chosen UUID and `isFreeAccount`, false for a paid Apple Developer account and true otherwise. It downloads the matching WebDriverAgent release, packages and resigns it with that profile, and returns a `capabilitiesHint`.
3. Pass that hint to `appium_session_management` (`action=create`), serialising the whole object without dropping its boolean or numeric values.

Two switches live on the phone and cannot be set from the Mac: Developer Mode, which `devicectl` reports, and **Settings -> Developer -> UI TESTING -> Enable UI Automation**, which it does not report at all. Without the second, session creation fails with a bare `xcodebuild failed with code 65`, and the real reason, "Timed out while enabling automation mode", appears only in the test log. Ask the user to turn both on and to leave the phone unlocked while a session runs. A paid account re-signs WebDriverAgent yearly; a free Apple ID's signature expires every 7 days, after which capture stops working until it is signed again.

### Simulator

A booted simulator is the whole requirement; there is no WebDriverAgent, provisioning profile or session to create.

```bash
"$SKILL_DIR/scripts/discover-ios-setup.mjs" --target simulator
```

Exit 0 prints the booted simulators and a `sessionDefaults` object. Exit 1 means nothing is booted, or several are and it will not guess: boot one with `boot_sim` (and `open_sim` if you want to watch), or choose one and re-run with `--device <udid>` to have it reported as selected. Keep the `selectedSimulator.udid` from that report as `$UDID`, then claim it:

```bash
"$SKILL_DIR/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID"
```

Exit 0 means it is yours. Exit 3 means another run holds it, and the message says which run and for how long: wait and claim again, or abort and pick another simulator. Never `--steal` unless you know that run is dead.

The plugin sets `XCODEBUILDMCP_DISABLE_SESSION_DEFAULTS=true`, so pass the claimed `simulatorId` on every simulator and UI tool call, `bundleId` on launch and stop calls, and the explicit project or workspace and scheme on build and app-path calls. Do not call `session_set_defaults` or switch active profiles, because those settings are shared across agents; the discovery report's `sessionDefaults` exists only for compatibility, so copy its selected UUID into calls instead. If the exposed schema omits `simulatorId`, reconnect the server with this plugin's configuration before parallel use. Compare each reported `artifacts.simulatorId`, or a snapshot's `udid`, with the chosen UUID and stop on a mismatch; never drive an ambiguous target.

For the same reason, never use `booted` in place of the UDID. `simctl` resolves it to one running simulator without saying which, and booted simulators routinely hold different builds of the same app. `claim-simulator.mjs` and `capture-slice.sh` refuse it.

### Interrupted runs

If a caller crashed or a create result is unknown, check Appium's session list and confirm the old run is no longer operating the phone. Only then recover the claim with `--steal`, which clears the old reservation, and delete any leftover session by its explicit `sessionId` before creating a new one. Do not delete lock files by hand; the OS releases them when their processes exit.

## Safety

On a real device you are driving someone's phone, often signed into a real account with real money; follow the same rules on a simulator.

- Stay read-only: never tap anything that transacts, sends, confirms, deletes, posts or follows.
- Never type into a credential, seed-phrase or payment field.
- Navigation, tab switching and scrolling are fine; anything that changes state is not.
- If a screen can be reached only by a state-changing action, stop and ask.

Never tap a coordinate without a fresh screenshot showing what is under it. An app resumes on whatever screen it was last left on, so remembered coordinates can land on something else entirely, such as a payment button. To dismiss a modal sheet, tap the dimmed backdrop above it; a downward swipe on the sheet body often does nothing.

## Open the app

On a real device, find the bundle id with the script, not with `devicectl` directly, because `xcrun devicectl device info apps` lists only developer-installed apps by default and an App Store app looks absent; the script passes `--include-all-apps`.

```bash
"$SKILL_DIR/scripts/find-ios-app.sh" --device <udid> --name <app name>
```

If no Appium session exists yet, create one: `select_device` (`platform=ios`, `iosDeviceType=real`, `deviceUdid=<udid>`), then `appium_session_management` with `action=create` and the discovered capabilities, which carry `appium:udid`. Foreground the app with `appium_app_lifecycle` (`action=activate`, `id=<bundleId>`). Sessions idle out: when a call fails with "Session does not exist", delete that session (`action=delete` with its explicit `sessionId`) and create again, because the gate denies a create while the claim still records the old session.

On a simulator, `devicectl` cannot see simulators, so the script reads `simctl listapps` instead:

```bash
"$SKILL_DIR/scripts/find-ios-app.sh" --simulator "$UDID" --name <app name>
```

It also accepts `booted`, but only when exactly one simulator is running; with several up it refuses.

Launch it with `launch_app_sim`. The screen can stay blank for several seconds, so wait with `wait_for_ui` (`predicate: settled`) before the first screenshot. A development build may resume on its dev launcher, a list of servers rather than the app: pick the running server, dismiss any developer menu, and confirm the app itself is on screen.

On either target, take a screenshot before navigating, because an app resumes where the user left it rather than on its home screen.

## Find the requested screen

Navigate by tab bar, search, or an element found with `appium_find_element` on a device or from a `snapshot_ui` target on a simulator, preferring `accessibility id` over xpath. Then look at a screenshot and confirm you are on the right screen before capturing; never assume a tap landed, since this is the most common way a run wastes its slices.

On the simulator some list rows are exposed as text rather than buttons, and tapping their ref fails with `TARGET_NOT_ACTIONABLE`; stock Settings is like this below its first level. Reach such a screen another way, or pick a different screen, rather than retrying the ref.

## Capture slices

Read these before the first scroll:

- **`direction` is the direction the content moves, not the finger**, on both targets. `direction=up` scrolls further down the page and `direction=down` moves toward the top. Getting it backwards produces slices that look random and overlap readings of "nothing moved".
- **Every scroll must name a container.** A screen can hold several scroll views, and a bare gesture may not move the page at all.
- **Scroll about half a screen.** One scoped scroll at the default distance advances roughly a full viewport, which leaves the stitcher no overlap to match and skips any short section between slices. Use `distance` 0.5 on the simulator and let the stitcher measure the real offset; on one screen 0.7 advanced 1819px of a 2220px body while 0.3 advanced 770px with a comfortable overlap. Do not hand-tune `x/y/endX/endY` drags, which frequently move nothing.

A floating scroll-to-top button returns to the top of the list, not the page, so expect one more `direction=down` scroll to bring a header or chart back into view.

### Capturing on a real device

Find a container with `appium_find_element` (`strategy=-ios class chain`, `selector=**/XCUIElementTypeScrollView`) and pass its `elementUUID` to every scroll. That selector returns the first match in hierarchy order, which may scroll the other axis, be nested or be inert, so after the first scroll compare against the previous frame and, if nothing moved, try `**/XCUIElementTypeScrollView[2]`, then `[3]` and so on. Exhausting them proves only that nothing scrolls vertically; see [One axis per capture](#one-axis-per-capture) before calling the screen one viewport tall.

`appium_screenshot` writes to the server's `SCREENSHOTS_DIR` or working directory and returns the path, not into `SLICE_DIR`. Copy each file across as you go, named so a glob sorts in capture order, and never pass `maxWidth`, because downscaling loses the detail the overlap matcher needs:

```bash
cp "$RETURNED_PATH" "$SLICE_DIR/slice-$(printf '%02d' "$N").png"
```

### Capturing on a simulator

Never capture slices with the XcodeBuildMCP `screenshot` tool: whatever the file is named, it returns a downscaled, lossy JPEG (369x800 for a 1179x2556 screen), fine for looking but useless for stitching. Capture with `capture-slice.sh`, which writes a full-resolution PNG straight into `SLICE_DIR` and refuses a simulator this run does not hold:

```bash
"$SKILL_DIR/scripts/capture-slice.sh" --simulator "$UDID" --run "$RUN_ID" \
  --out "$SLICE_DIR/slice-$(printf '%02d' "$N").png"
```

Exit 0 prints the path it wrote; exit 2 is a bad argument, `booted` included; exit 3 is a simulator this run does not hold, unclaimed or held by another run the message names, which is the one-agent-per-simulator rule and not a reason to retry.

Scroll with `swipe`, which requires `withinElementRef`. Take the first ref from `snapshot_ui`, which lists scrollable targets, and call `snapshot_ui` again after every swipe for the next one. Never reuse a ref across a swipe because it worked before: a top-level container often keeps the same ref string for a swipe or two and then changes without warning, and a stale ref fails with `TARGET_NOT_ACTIONABLE` or `SNAPSHOT_MISSING`. On either error take a new snapshot rather than retrying. If a swipe response does carry targets, use them and skip the snapshot call.

Most swipes warn `SNAPSHOT_CAPTURE_FAILED`, "the refreshed runtime snapshot did not settle". That is the accessibility tree timing out on a busy screen, not the rendering: the pixels are fine, so take a fresh `snapshot_ui` and carry on. No delay is needed between a swipe and the capture, because the screen has stopped moving by the time `swipe` returns. `snapshot_ui` reports no geometry at all, so anything that needs to know where a region sits must read it from the pixels, which is what the stitcher's `--crop-band` does.

### One axis per capture

The capture loop scrolls vertically and the stitcher joins along one axis. Before concluding a screen does not scroll, try a horizontal scroll on the same containers.

- **A screen that only scrolls sideways**, such as a wide table or a paged gallery, is captured the same way, scrolling `direction=left` to advance, and stitched with `--axis horizontal`. Fixed chrome is then read off the left and right edges, so `--sticky-top` and `--sticky-bottom` mean left and right.
- **A screen that scrolls both ways** cannot become one image. Capture the vertical page as the main artifact and each horizontally scrollable region as its own image named for that region. Never assemble a two-dimensional mosaic: the matcher aligns along a single axis and a grid gives it no consistent seam.

A sideways region must be reduced to its own band before stitching. The rest of the screen holds still while a carousel scrolls, and static content matches at any offset, so full-screen slices splice confidently in the wrong place.

- **Real device:** pass the container's `elementUUID` to `appium_screenshot` to crop the capture to that band. Find the container by shape: walk `**/XCUIElementTypeScrollView[1]`, `[2]`, … and read each one's `appium_get_element_attribute` (`attribute=rect`). A horizontal scroller is full screen width and a fraction of its height, such as 393x118, while a page container is nearly screen height, such as 393x704. The stitcher trims element captures that differ by a pixel to their common size.
- **Simulator:** capture full screen and stitch with `--axis horizontal --crop-band`, which keeps the rows that change between slices and reports them as `cropped_band`. Check that against the carousel you meant to capture.

### Settle, then capture

Let the screen settle before the first slice, because a frame taken mid-transition differs from the same screen a moment later and reads as scrolling, which can stitch a non-scrolling screen to itself. Screenshot twice and start only once two consecutive frames are nearly identical, a `mean_abs_diff` below about 2. Bound that wait to about three attempts: a live feed never settles. A transition's difference drops to near zero within a second or two, while live content holds a steady difference; once you know it is live, capture anyway and say so in the report.

Compare two frames with `frame_diff.py`; the system python has no imaging library, so do not reach for another tool.

```bash
"$SKILL_DIR/scripts/frame_diff.py" <before.png> <after.png>
```

It prints `mean_abs_diff` on a 0-255 scale and `scrolled_px`, the offset at which the later frame's content is found in the earlier one, with its `match_error`, and it detects fixed chrome from the pair as `sticky_top` and `sticky_bottom`; pass those flags only to override it. Judge movement by `scrolled_px`, not by the difference: a net worth, PnL or balance ticks on its own, so a static screen never reaches zero difference, while `scrolled_px` stays 0 when nothing scrolled and a real scroll reports its offset with a low `match_error`.

Name every throwaway frame (settle probes, the at-the-top check, the bottom marker) `probe-*.png` in `SLICE_DIR`, so the `slice-*.png` glob cannot pick one up. The stitcher refuses an identical pair, but a probe taken mid-transition is not identical to anything and slips through.

Then run the capture loop:

1. Scroll toward the top (`direction=down`) until it stops changing: capture a probe, swipe once more, capture another probe, and compare. `scrolled_px` 0 means you are at the top; an app usually resumes near the top of a tab, so this is often one swipe.
2. Capture slice 1, a fresh capture named as a slice, not one of the probes.
3. Scroll `direction=up` once and capture the next slice.
4. Repeat until the page stops moving, with a hard stop at 6 slices.

When a new frame shows the page did not move (difference below 2, or `scrolled_px` 0), stop: that frame is the bottom marker, so capture it as a probe, never a slice. If that happens on the very first scroll, the screen does not scroll at all and one slice is the whole screen; pass it alone to the stitcher, which copies it through, and never stitch a screen to itself.

Judge what is advancing. Repeating rows of the same shape, such as a comment thread, feed or search results, are an endless list: one scroll and two slices are enough, because the stitched image already shows the endless section, so stop there and report the capture as truncated. Distinct sections that each appear once, such as a description, a stats table and a footer, are finite page content: follow them to the bottom.

A live feed cannot be captured as one coherent page. New rows arrive between slices, so the image is a composite of moments: row ages will not read in order, a "new items" affordance may appear mid-image, and seam error runs close to the accept threshold. Present it as a composite, not as the screen at one instant.

## Stitch

Pass the slices in capture order; the stitcher trusts the order it is given and produces a confidently wrong image from slices out of order.

```bash
"$SKILL_DIR/scripts/stitch_screens.py" \
  --out "$OUT_ROOT/<app-slug>/<screen-slug>.png" \
  --slices "$SLICE_DIR"/slice-*.png
```

It detects the fixed chrome (status bar, sticky header, pinned bottom bar), finds where each slice overlaps the previous one, and splices so duplicated content appears once. Read its JSON verdict:

- Every seam must say `"spliced": true`, and `all_spliced` must be true. A `"butt_joined"` seam means no overlap was found and content may be missing there.
- `vs_previous_diff` per seam separates "never moved" from "moved but would not align".
- `replaced_existing` reports that this run replaced an earlier capture of the same screen.
- The stitcher writes nothing when two consecutive slices are identical: that is a bottom marker that should have been a probe, or a swipe that hit a different simulator than the capture. Fix the slices; do not pass the pair some other way.

Where a butt-joined seam sits tells you what went wrong. At the bottom of the page, the last scroll usually hit the end and moved almost nothing; a `vs_previous_diff` near zero means that slice was not content and should have been discarded. Mid-page, one swipe advanced further than a screen, so whatever sat between the two slices was never captured and the image looks plausible while missing a section; recapture that stretch with about half the `distance`. This is the failure most easily mistaken for a good capture.

Never chase a failing seam by raising `--max-error`. Loosening the threshold accepts a worse alignment rather than finding a better one: on one six-slice capture it made every seam report spliced and produced an image a third shorter than the page, with the missing rows gone silently. A failing seam almost always means wrong chrome bounds, so check `sticky_detected` first, and if it looks wrong override it and re-run. Both flags take the number of pixels of fixed chrome at each edge, not row indices; read the status bar plus any pinned header, and the pinned bottom bar, off a slice:

```bash
--sticky-top 362 --sticky-bottom 357
```

Detection handles a pinned header with a live-updating value, and with three or more slices it ignores the first, because iOS expands a large navigation title at the top of a page and collapses it once the page moves. With exactly two slices it must use the first, so where the title collapses the band reads as content and the crop stops short of the title bar; a compact title bar that does not change detects fine. That failure is loud, `all_spliced` false with the slices butt-joined, and the fix is to read the chrome height off a slice and pass `--sticky-top`, not to raise `--max-error`.

Then open the result and check continuity across seams: ordered lists stay ordered and no row repeats. On a dark UI a flat black band can match anywhere, so a low error alone is not proof. When a stitch looks suspect, check its height: it should be about the first slice plus the sum of the scroll steps, minus the bottom chrome that the first slice carries and the stitch drops; far short means content was dropped whatever `all_spliced` says. That arithmetic cannot catch a bad butt join, which pads in a whole untrimmed slice and comes out longer than predicted while a section is missing, so for a butt-joined seam read the bottom of the earlier slice and the top of the next and ask whether anything belongs between them.

A button floating over the page, rather than flush with an edge, cannot be removed, because chrome is detected only at the edges. It is spliced in as content and may repeat: one pinned near the bottom usually lands in the discarded tail and shows once, while one over the middle can show once per slice. That is a property of the screen, not a bad stitch, so say so in the report rather than re-running.

## Clean up

Delete `SLICE_DIR`, on a phone delete this run's Appium session by its explicit `sessionId`, then release the claim:

```bash
"$SKILL_DIR/scripts/claim-simulator.mjs" "$UDID" --run "$RUN_ID" --release
```

The stitched PNG is the only artifact that survives. Name it for what the screen shows, such as `settings.png`, `search-results.png` or `product-detail.png`, never `screenshot-1.png` or a timestamp. Where a screen's header and the tab that reaches it disagree, such as an "Account" tab above a page headed "Portfolio", name it for the header. The app slug is the app's display name from `find-ios-app.sh`, lowercased with spaces turned to hyphens: `MyApp` becomes `myapp`.

## Reporting

State the target, the full output path, the number of slices, which axis was captured and whether content extends past it, whether the capture was truncated by infinite scroll or is a live-feed composite, any floating button that repeats, and every seam's status. If any seam was butt-joined, say so plainly instead of presenting the image as complete.

## Tests

```bash
npm run test:mobile
```
