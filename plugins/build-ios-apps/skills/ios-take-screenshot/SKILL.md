---
name: ios-take-screenshot
description: Capture one whole iOS app screen as a single stitched PNG, including everything below the fold. Use when asked to screenshot an app screen, capture a full page, or collect screens of another app for design research. Drives a real iPhone through appium-mcp, or the simulator through XcodeBuildMCP.
---

# iOS Take Screenshot

Produce exactly ONE image per requested screen. iOS has no full-page screenshot API — `screenshot` returns only the visible viewport — so a whole screen must be captured as slices and stitched. This skill owns that end to end: open the app, reach the screen, capture slices, stitch, delete the slices.

## Where Results Go

Working slices go in a per-run temp directory and are always deleted:

```bash
SKILL_DIR="<absolute path to this loaded skill folder>"
if [ -z "${SLICE_DIR:-}" ]; then
  SLICE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/ios-screenshot.XXXXXX")"
fi
mkdir -p "$SLICE_DIR"
```

The stitched PNG goes where the caller asks, via `--out`. `IOS_SCREENSHOT_DIR` is an
ordinary environment variable, so it is shared by every session that inherits the same
shell. Give each run its own subdirectory, so two sessions capturing the same screen cannot
overwrite each other:

```bash
RUN_ID="$(date +%Y%m%d-%H%M%S)-$$"
OUT_ROOT="${IOS_SCREENSHOT_DIR:-/tmp/build-ios-app}/$RUN_ID"
```

Each screen is then named for what it shows:

```
$OUT_ROOT/<app-slug>/<screen-slug>.png
```

Report the full path when you finish — a run-scoped directory is only useful to later tools
if they are told where it is. A path given in the request always wins over the default.

The default root lives under `/tmp`, which macOS clears on reboot. Point
`IOS_SCREENSHOT_DIR` at a durable directory for screens worth keeping.

Several agents can share that library safely. The phone is the real lock, not the directory: Appium holds one session per device, so two agents cannot capture at the same time — the second fails to get a session. The stitcher writes to a staging file and renames it into place, so a reader never sees a half-written PNG, and its verdict reports `replaced_existing` when a run overwrites an earlier capture of the same screen.

## Core Workflow

0. Discover the device and its session capabilities; set up WebDriverAgent if absent.
1. Open the app (find it first — the default app listing hides App Store apps).
2. Navigate to the requested screen and confirm you are on it by looking at a screenshot.
3. Scroll to the top, then capture overlapping slices downward, capped.
4. Stitch with `scripts/stitch_screens.py` and read its JSON verdict.
5. Delete the slices. Keep only the stitched PNG.

`SKILL_DIR` is the absolute path of this loaded skill folder. Do not derive it from the target app's `pwd` — installed plugins live outside the app being researched. Keep slices in a run-specific temp dir, never under `SKILL_DIR`.

## Tool Names

The plugin serves the Appium tools, so they carry its prefix:
`appium_screenshot` is `mcp__plugin_build-ios-apps_appium-mcp__appium_screenshot`, and so
on for every `appium_*` name below. They are written unprefixed here for readability.

Device-specific capabilities — UDID, team id, WebDriverAgent bundle id — are not in the
plugin config, since they differ per machine. Pass them inline to
`appium_session_management` (`action=create`), or point the server at a local
`capabilities.json` with `CAPABILITIES_CONFIG`. Do not ask the user for these values and
do not carry them between sessions — step 0 discovers them and prints them ready to use.

## Safety

You are driving someone's real phone, often signed into a real account with real money.

- Read-only. Never tap anything that transacts, sends, confirms, deletes, posts, or follows.
- Never type into a credential, seed-phrase, or payment field.
- Navigation, tab switching, and scrolling are fine. Anything that changes state is not.
- If a screen can only be reached by a state-changing action, stop and ask.

**Never tap a coordinate without a fresh screenshot showing what is under it.** An app
resumes on whatever screen it was last left on, so coordinates memorised from an earlier
run land somewhere else entirely. Tapping a remembered tab-bar position after reopening
one app hit a "Deposit to buy" button and opened a payment sheet. Screenshot, look, then
tap.

To dismiss a modal sheet, tap the dimmed backdrop above it. A downward swipe on the sheet
body often does nothing, and repeating it wastes turns.

## 0. Set Up the Device

Discover the session values rather than asking for them or remembering them:

```bash
"$SKILL_DIR/scripts/discover_ios_setup.py"
```

It reports the connected devices, which provisioning profiles cover them, whether
WebDriverAgent is installed, and — when everything is in place — a `suggestedCapabilities`
object to pass straight to `appium_session_management` (`action=create`). Exit 0 means
ready; exit 1 lists what is missing. The UDID comes from the device list, the team id from
a profile that covers the device, and the WebDriverAgent bundle id from the runner already
installed on it.

**If WebDriverAgent is not installed, do not build it by hand.** Use
`appium_prepare_ios_real_device`:

1. Call it with no `provisioningProfileUuid` to list available profiles.
2. Call it again with the chosen UUID and `isFreeAccount` — false for a paid Apple
   Developer account, true otherwise. It downloads the matching WebDriverAgent release,
   packages it as an IPA, resigns it with that profile, and returns a `capabilitiesHint`.
3. Pass that hint to `appium_session_management` (`action=create`), serialising the whole
   object — do not drop its boolean or numeric values.

Two switches live on the phone and cannot be set from the Mac. Developer Mode, which
`devicectl` does report, and **Settings -> Developer -> UI TESTING -> Enable UI
Automation**, which it does not report at all. Without the second, session creation fails
with a bare `xcodebuild failed with code 65`, and the real reason — "Timed out while
enabling automation mode" — appears only in the test log. Ask the user to turn both on, and
to leave the phone unlocked while a session runs.

A paid Apple Developer account re-signs WebDriverAgent yearly; a free Apple ID expires it
every 7 days, after which capture stops working until it is signed again.

## 1. Open the App

On a real device, resolve the bundle id first:

```bash
"$SKILL_DIR/scripts/find_ios_app.sh" --device <udid> --name fomo
```

`xcrun devicectl device info apps` lists **only developer-installed apps by default** — an App Store app looks absent. The script passes `--include-all-apps`, which is the whole reason it exists. Do not call `devicectl` directly for this.

Then foreground it:

- Real device: `appium_app_lifecycle` with `action=activate`, `id=<bundleId>`
- Simulator: `mcp__plugin_build-ios-apps_xcodebuildmcp__launch_app_sim`

Then screenshot. An app resumes where the user left it, not on its home screen, so confirm
where you actually are before navigating.

If no Appium session exists yet, create one: `select_device` (`platform=ios`, `iosDeviceType=real`, `deviceUdid=<udid>`), then `appium_session_management` with `action=create`. Sessions idle out — just recreate on failure.

## 2. Find the Requested Screen

Navigate by tab bar, search, or an element found with `appium_find_element`. Prefer `accessibility id` over xpath.

Then **look at a screenshot and confirm you are on the right screen** before capturing. Do not assume a tap landed. This is the single most common way a capture run wastes its slices.

## 3. Capture Slices

Read this section before your first scroll. These three facts cost an hour to learn:

- **`direction` is the direction the CONTENT moves, not the finger.** `direction=up` scrolls you FURTHER DOWN the page. To move toward the top of a page, use `direction=down`. Getting this backwards produces slices that look random and overlap measurements that read as "nothing moved".
- **Scoping matters.** On a nested scroll view, a bare `appium_gesture` may not move the page at all. Find the container once and reuse it:
  `appium_find_element` with `strategy=-ios class chain`, `selector=**/XCUIElementTypeScrollView`, then pass its `elementUUID` to every scroll.
- **One scoped scroll advances roughly a full viewport.** That is fine — the stitcher measures the real offset. Do not hand-tune drag coordinates; scoped `direction` scrolls are far more reliable than custom `x/y/endX/endY` drags, which frequently move nothing.

A floating scroll-to-top button, where an app has one, returns to the top of the *list*, not the top of the *page*. Expect one more `direction=down` scroll to bring a header or chart back into view.

**Let the screen settle before the first slice.** A screen captured mid-transition differs
from the same screen a moment later, and that difference is easily mistaken for scrolling.
Screenshot twice and compare; only start capturing once two consecutive frames are nearly
identical. Skipping this produced a run that captured one non-scrolling screen twice and
stitched a duplicate.

**Bound that wait to about three attempts.** A live feed never settles — its content keeps
arriving — so an unbounded settle loop waits forever. Tell the two apart by whether the
change decays: a transition drops to near zero within a second or two, while live content
holds a steady difference indefinitely. Once you have established it is live, capture
anyway and say so in the report.

Capture loop:

1. Scroll toward the top (`direction=down`) until the top no longer changes.
2. Screenshot → slice 1.
3. Scroll `direction=up` once → screenshot → next slice.
4. Repeat until the page stops moving, with a hard stop at **6 slices**.

"Nearly identical" means a mean absolute pixel difference below about 2 on settled frames.
Compare each new slice against the previous one:

- **Below 2 → stop.** The page did not move. Discard that slice; it marks the bottom, it is
  not content.
- **If that happens on the very first scroll, the screen does not scroll at all.** One slice
  is the whole screen. Pass it alone to the stitcher, which copies a single slice through
  unchanged. Do not stitch a screen to itself.

The stitcher reports `vs_previous_diff` per seam, which separates "never moved" from "moved
but would not align" when a seam fails.

**Infinite scroll: two screens is enough — one scroll.** An endless list has no bottom to
reach, and capturing more of it adds rows, not information. Such a list is usually already
partly visible on the first screen, so a single scroll reveals the next page of rows, and
the stitched image makes the endless section obvious. Stop there and report the capture as
truncated.

Judge which case you are in by what is advancing. Repeating rows of the same shape — a
holders list, a feed, a leaderboard — are an endless list: stop at two screens. Distinct
sections that each appear once — a description, a stats table, a footer — are finite page
content: follow them to the bottom.

**A live feed cannot be captured as one coherent page.** New rows arrive between slices, so
the stitched image is a composite of two moments rather than a snapshot of one: row ages
will not read in order, and a "new activity" pill may appear mid-image. Seam error also
runs close to the accept threshold, because no two frames of a live screen match cleanly.
Present such a capture as a composite, not as the state of the screen at one instant.

Save slices at full resolution — do not pass `maxWidth` when capturing for a stitch, since downscaling loses the detail the overlap matcher needs.

`appium_screenshot` writes wherever the MCP server is configured to write (`SCREENSHOTS_DIR`, otherwise the working directory) and returns that path. It does not write into `SLICE_DIR`. Copy each returned file across as you go, named so a glob sorts in capture order:

```bash
cp "$RETURNED_PATH" "$SLICE_DIR/slice-$(printf '%02d' "$N").png"
```

The stitcher trusts the order it is given; passing slices out of order produces a confidently wrong image.

## 4. Stitch

```bash
"$SKILL_DIR/scripts/stitch_screens.py" \
  --out "$OUT_ROOT/fomo/token-detail.png" \
  --slices "$SLICE_DIR"/slice-*.png
```

The script auto-detects the fixed chrome (status bar, sticky header, pinned bottom bar), finds where each slice overlaps the previous one by sliding a textured band and minimising pixel difference, and splices at the matched row so duplicated content appears once.

It prints a JSON verdict. **Read it.** Every seam must say `"spliced": true`. A `"butt_joined"` seam means no overlap was found and content may be missing at that seam.

If `sticky_detected` in the verdict looks wrong, override it and re-run. Both flags take the **number of pixels** of fixed chrome at each edge, not row indices:

```bash
--sticky-top 362 --sticky-bottom 357
```

Read those off a slice: how tall is the status bar plus any pinned header, and how tall is the pinned bottom bar. Detection handles a pinned header that shows a live-updating value, because it measures the share of pixels in a row that change rather than the size of the change.

Verify the result by opening it and checking continuity across seams: ordered lists must stay ordered, and no row may repeat. On a dark UI a flat black band can match anywhere, so a low error score alone is not proof.

## 5. Clean Up

Delete the slice directory. The stitched PNG is the only artifact that survives. Name it for what it shows — `token-detail.png`, `holders-tab.png`, `perps-list.png` — never `screenshot-1.png` or a timestamp.

## Tests

`scripts/test_stitch_screens.py` builds a synthetic page, cuts it into overlapping
slices with fixed chrome on both edges — including a pinned header carrying a
live-updating value — and asserts the stitch reconstructs the page row for row.
Run it after any change to the stitcher.

## Reporting

State the output path, the number of slices, whether the capture was truncated by infinite scroll, and every seam's status. If any seam was butt-joined, say so plainly instead of presenting the image as complete.
