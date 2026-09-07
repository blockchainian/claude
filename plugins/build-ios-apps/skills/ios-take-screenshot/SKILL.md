---
name: ios-take-screenshot
description: Capture one whole iOS app screen as a single stitched PNG, including everything below the fold. Use when asked to screenshot an app screen, capture a full page, or collect screens of another app for design research. Drives either a real iPhone connected over USB, through appium-mcp, or a booted simulator, through XcodeBuildMCP.
---

# iOS Take Screenshot

Produce exactly ONE image per requested screen. iOS has no full-page screenshot API — `screenshot` returns only the visible viewport — so a whole screen must be captured as slices and stitched. This skill owns that end to end: open the app, reach the screen, capture slices, stitch, delete the slices.

## Pick the Target First

Everything below has a real-device path and a simulator path. They differ in three places
only — how the app is found, how a session is set up, and which calls scroll and capture.
The stitcher, the output convention, the slice discipline, the settle rule, the
endless-list rule, cleanup and reporting are identical.

| | Real device | Simulator |
|---|---|---|
| App lookup | `devicectl` | `simctl` |
| Drive layer | appium-mcp | XcodeBuildMCP |
| Capture | `appium_screenshot` | `xcrun simctl io` |

Use the device when the request names one, when the app is only installed on a phone, or
when the point is how the app behaves on real hardware. Use the simulator when the app is
already running there, when no phone is connected, or when the request is about layout and
content rather than the device. If the request does not say and both are available, ask.

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

Several agents can share that library safely. On a device the phone is the real lock, not
the directory: Appium holds one session per device, so two agents cannot capture at the
same time — the second fails to get a session. A simulator has no such lock, so two agents
driving the same booted simulator will fight over what is on screen; capture one screen at
a time. The stitcher writes to a staging file and renames it into place, so a reader never
sees a half-written PNG, and its verdict reports `replaced_existing` when a run overwrites
an earlier capture of the same screen.

## Core Workflow

0. Discover the target and set up the session.
1. Open the app (find it first — the default device listing hides App Store apps).
2. Navigate to the requested screen and confirm you are on it by looking at a screenshot.
3. Scroll to the top, then capture overlapping slices downward, capped.
4. Stitch with `scripts/stitch_screens.py` and read its JSON verdict.
5. Delete the slices. Keep only the stitched PNG.

`SKILL_DIR` is the absolute path of this loaded skill folder. Do not derive it from the target app's `pwd` — installed plugins live outside the app being researched. Keep slices in a run-specific temp dir, never under `SKILL_DIR`.

## Tool Names

The plugin serves both MCP servers, so their tools carry its prefix. `appium_screenshot` is
`mcp__plugin_build-ios-apps_appium-mcp__appium_screenshot`, and `swipe` is
`mcp__plugin_build-ios-apps_xcodebuildmcp__swipe`. They are written unprefixed below for
readability.

Device-specific capabilities — UDID, team id, WebDriverAgent bundle id — are not in the
plugin config, since they differ per machine. Pass them inline to
`appium_session_management` (`action=create`), or point the server at a local
`capabilities.json` with `CAPABILITIES_CONFIG`. Do not ask the user for these values and
do not carry them between sessions — step 0 discovers them and prints them ready to use.

## Safety

On a real device you are driving someone's phone, often signed into a real account with
real money. A simulator has no real account behind it, but the same discipline keeps a
capture run honest and cheap, so follow it on both.

- Read-only. Never tap anything that transacts, sends, confirms, deletes, posts, or follows.
- Never type into a credential, seed-phrase, or payment field.
- Navigation, tab switching, and scrolling are fine. Anything that changes state is not.
- If a screen can only be reached by a state-changing action, stop and ask.

**Never tap a coordinate without a fresh screenshot showing what is under it.** An app
resumes on whatever screen it was last left on, so coordinates memorised from an earlier
run land somewhere else entirely. In one run, a remembered tab-bar position landed on a
payment button after the app reopened elsewhere, opening a checkout sheet. Screenshot,
look, then tap.

To dismiss a modal sheet, tap the dimmed backdrop above it. A downward swipe on the sheet
body often does nothing, and repeating it wastes turns.

## 0. Set Up the Target

### Real device

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

### Simulator

There is no WebDriverAgent, no provisioning profile and no session to create. A booted
simulator is the whole requirement:

```bash
"$SKILL_DIR/scripts/discover_ios_setup.py" --target simulator
```

Exit 0 prints the booted simulators and a `sessionDefaults` object; exit 1 says nothing is
booted. Pass that UDID to `session_set_defaults` once, and every XcodeBuildMCP call after
it targets that simulator. Boot one with `boot_sim` if none is running, and `open_sim` if
you want to watch.

## 1. Open the App

### Real device

```bash
"$SKILL_DIR/scripts/find_ios_app.sh" --device <udid> --name <app name>
```

`xcrun devicectl device info apps` lists **only developer-installed apps by default** — an App Store app looks absent. The script passes `--include-all-apps`, which is the whole reason it exists. Do not call `devicectl` directly for this.

Foreground it with `appium_app_lifecycle` (`action=activate`, `id=<bundleId>`), then
screenshot. An app resumes where the user left it, not on its home screen, so confirm
where you actually are before navigating.

If no Appium session exists yet, create one: `select_device` (`platform=ios`, `iosDeviceType=real`, `deviceUdid=<udid>`), then `appium_session_management` with `action=create`. Sessions idle out — just recreate on failure.

### Simulator

```bash
"$SKILL_DIR/scripts/find_ios_app.sh" --simulator booted --name <app name>
```

`devicectl` cannot see simulators at all, so this reads `simctl listapps` instead. `booted`
works in place of a UDID when exactly one simulator is running.

Then `launch_app_sim` with that bundle id, and screenshot to see where the app resumed.

## 2. Find the Requested Screen

Navigate by tab bar, search, or an element found with `appium_find_element` on a device, or
from a `snapshot_ui` target on a simulator. Prefer `accessibility id` over xpath.

Then **look at a screenshot and confirm you are on the right screen** before capturing. Do not assume a tap landed. This is the single most common way a capture run wastes its slices.

Not every visible row can be tapped by ref. On the simulator some list rows are exposed as
text rather than buttons, and tapping their ref fails with `TARGET_NOT_ACTIONABLE`; stock
Settings is like this below its first level. Reach such a screen another way, or pick a
different screen, rather than retrying the same ref.

## 3. Capture Slices

Read this section before your first scroll. These three facts cost an hour to learn:

- **`direction` is the direction the CONTENT moves, not the finger.** `direction=up` scrolls you FURTHER DOWN the page. To move toward the top of a page, use `direction=down`. This holds on both targets. Getting it backwards produces slices that look random and overlap measurements that read as "nothing moved".
- **Scoping matters, and a screen can hold several scroll views.** A bare gesture may not
  move the page at all, so every scroll must name a container.
- **One scoped scroll advances roughly a full viewport** at the default distance, which is
  too far. The stitcher joins slices by finding where they overlap, so a scroll that
  advances a whole screen leaves nothing to match on, and a short section between two
  slices is never photographed at all. Ask for about half a screen — `distance` 0.5 on the
  simulator — and let the stitcher measure the real offset. Measured on one screen:
  `distance` 0.7 advanced 1819px of a 2220px body, over 80%, while 0.3 advanced 770px and
  left a comfortable overlap. Do not hand-tune drag coordinates; scoped `direction` scrolls
  are far more reliable than custom `x/y/endX/endY` drags, which frequently move nothing.

A floating scroll-to-top button, where an app has one, returns to the top of the *list*, not the top of the *page*. Expect one more `direction=down` scroll to bring a header or chart back into view.

### Scrolling and capturing on a real device

Find a container with `appium_find_element` (`strategy=-ios class chain`,
`selector=**/XCUIElementTypeScrollView`) and pass its `elementUUID` to every scroll.
That selector returns the **first** match in hierarchy order, which is not necessarily the
one holding the content you want. It may scroll a different axis, or be nested, or be
inert. So after the first scroll, compare against the previous frame: if nothing moved,
try `**/XCUIElementTypeScrollView[2]`, then `[3]`, and so on. Exhausting them establishes
only that nothing moves the screen **vertically** — see below before calling the screen
one viewport tall.

`appium_screenshot` writes wherever the MCP server is configured to write (`SCREENSHOTS_DIR`, otherwise the working directory) and returns that path. It does not write into `SLICE_DIR`. Copy each returned file across as you go, named so a glob sorts in capture order:

```bash
cp "$RETURNED_PATH" "$SLICE_DIR/slice-$(printf '%02d' "$N").png"
```

Save slices at full resolution — do not pass `maxWidth` when capturing for a stitch, since downscaling loses the detail the overlap matcher needs.

### Scrolling and capturing on a simulator

**Do not capture slices with the XcodeBuildMCP `screenshot` tool.** It returns a downscaled,
lossy JPEG — 369x800 for a screen that is really 1179x2556 — whatever the file is named. It
is fine for looking at a screen; it destroys the detail the overlap matcher needs. Capture
with `simctl` instead, which writes a full-resolution PNG straight into `SLICE_DIR`, so
there is no copy step:

```bash
xcrun simctl io <udid> screenshot --type=png "$SLICE_DIR/slice-$(printf '%02d' "$N").png"
```

Scroll with `swipe`, which requires `withinElementRef`. Get the first ref from `snapshot_ui`
— it lists scrollable targets — and then **take each next ref out of the previous `swipe`
response**, which carries a fresh snapshot of its own. Refs go stale as soon as the screen
moves, and a stale one fails with `TARGET_NOT_ACTIONABLE`. A top-level container sometimes
keeps the same ref string across several swipes and a nested one does not, so read it from
the latest response every time rather than assuming.

No delay is needed between the swipe and the capture: by the time `swipe` returns, the
screen has stopped moving. Expect `swipe` to warn `SNAPSHOT_CAPTURE_FAILED` — "the
refreshed runtime snapshot did not settle" — on most calls; that is the norm on a busy
screen, and it is its accessibility tree timing out, not the rendering. Take a fresh `snapshot_ui` for the next ref and carry on; the pixels are fine.

`snapshot_ui` reports no geometry at all — no rect, no frame, no coordinates. Anything that
needs to know where a region sits must read it from the pixels instead. That is what
`--crop-band` on the stitcher is for; see the sideways capture below.

### One capture covers one axis

The loop below scrolls vertically, and the stitcher joins slices along that axis. Before
concluding a screen does not scroll at all, try a horizontal scroll on the same containers:

- **A screen that only scrolls sideways** — a wide table, a paged gallery — moves on the
  horizontal attempt. Capture it the same way, scrolling `direction=left` to advance, and
  stitch with `--axis horizontal`. Fixed chrome is then read off the left and right edges,
  so `--sticky-top` and `--sticky-bottom` mean left and right if you need to override them.
- **A screen that scrolls both ways** cannot become one image. Capture the vertical page as
  the main artifact, then capture any horizontally scrollable region as its own image named
  for that region. Do not try to assemble a two-dimensional mosaic: the overlap matcher
  aligns along a single axis, and a grid of slices gives it no consistent seam to find.

**A sideways region must be reduced to its own band before stitching.** A carousel occupies
a band; the rest of the screen holds still while it scrolls. Full-screen slices would be
mostly static, and static content matches at any offset, so the matcher splices confidently
in the wrong place. There are two ways to get the band, one per target:

- **Real device:** pass the container's `elementUUID` to `appium_screenshot` and the capture
  is cropped to that band. Find the container by shape rather than by guessing an index.
  Walk `**/XCUIElementTypeScrollView[1]`, `[2]`, … and read each one's geometry with
  `appium_get_element_attribute` (`attribute=rect`). A horizontal scroller is wide and
  short — full screen width, a fraction of its height — while a page container is nearly as
  tall as the screen. On one app this immediately separated a 393x118 carousel from the
  393x704 page container, with no trial-and-error scrolling.
- **Simulator:** there is no element-scoped capture and no geometry to crop to, so capture
  full screen and let the stitcher find the band: `--axis horizontal --crop-band`. It keeps
  the rows that change between slices, which is exactly the region that scrolled, and
  reports them as `cropped_band` in the verdict. Check that against the carousel you meant
  to capture.

Element captures can differ by a pixel between frames as the rect rounds; the stitcher trims
to the common size rather than rejecting the set.

Either way, say in the report which axis was captured and whether content extends past it.
A capture that silently drops the other axis reads as complete when it is not.

### Settle, then capture

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

Two slices is also the one case where chrome detection cannot help itself. It works by
comparing slices, and with a single pair the first slice's expanded navigation title is
half the evidence, so it reads as content and the crop stops short of the title bar. The
seam then fails loudly rather than silently — `all_spliced` is false and the slices are
butt-joined. If that happens on a two-slice capture, read the chrome height off a slice and
pass `--sticky-top` explicitly; do not reach for `--max-error`.

Judge which case you are in by what is advancing. Repeating rows of the same shape — a
comment thread, a feed, a search-results list — are an endless list: stop at two screens. Distinct
sections that each appear once — a description, a stats table, a footer — are finite page
content: follow them to the bottom.

**A live feed cannot be captured as one coherent page.** New rows arrive between slices, so
the stitched image is a composite of two moments rather than a snapshot of one: row ages
will not read in order, and a "new items" affordance may appear mid-image. Seam error also
runs close to the accept threshold, because no two frames of a live screen match cleanly.
Present such a capture as a composite, not as the state of the screen at one instant.

The stitcher trusts the order it is given; passing slices out of order produces a confidently wrong image.

## 4. Stitch

```bash
"$SKILL_DIR/scripts/stitch_screens.py" \
  --out "$OUT_ROOT/<app-slug>/<screen-slug>.png" \
  --slices "$SLICE_DIR"/slice-*.png
```

The script auto-detects the fixed chrome (status bar, sticky header, pinned bottom bar), finds where each slice overlaps the previous one by sliding a textured band and minimising pixel difference, and splices at the matched row so duplicated content appears once.

It prints a JSON verdict. **Read it.** Every seam must say `"spliced": true`. A
`"butt_joined"` seam means no overlap was found and content may be missing at that seam.

Where that seam sits tells you what went wrong:

- **At the bottom of the page**, it usually means the last scroll hit the end and moved
  almost nothing. Check `vs_previous_diff`: near zero means the slice was not content and
  should have been discarded.
- **Mid-page**, the scroll overshot. One swipe advanced further than a screen, so the two
  slices do not overlap and whatever sat between them was never captured — the stitched
  image looks plausible and is missing a section. Recapture that stretch with a smaller
  `distance`, roughly half of what you used. This is the failure most likely to be
  mistaken for a good capture, because nothing about the image looks wrong.

**Do not chase a failing seam by raising `--max-error`.** Loosening the threshold does not
find a better alignment; it accepts a worse one. On a six-slice capture that failed one
seam, raising it made every seam report `"spliced": true` and produced an image a third
shorter than the page it came from, with the missing rows gone silently. When a seam fails,
the cause is almost always the chrome bounds, so check those first.

If `sticky_detected` in the verdict looks wrong, override it and re-run. Both flags take the **number of pixels** of fixed chrome at each edge, not row indices:

```bash
--sticky-top 362 --sticky-bottom 357
```

Read those off a slice: how tall is the status bar plus any pinned header, and how tall is the pinned bottom bar. Detection handles a pinned header that shows a live-updating value, because it measures the share of pixels in a row that change rather than the size of the change. It also ignores the first slice when three or more were captured, because iOS expands a large navigation title at the top of a page and collapses it as soon as the page moves — measuring that band against later slices reads it as content and crops short of it. With exactly two slices there is nothing left to measure once the first is set aside, so that screen needs `--sticky-top` passed by hand.

Verify the result by opening it and checking continuity across seams: ordered lists must stay ordered, and no row may repeat. On a dark UI a flat black band can match anywhere, so a low error score alone is not proof.

One thing the stitcher cannot remove: a button that floats over the middle of the page
rather than sitting at an edge. Chrome is detected at the top and bottom edges only, so a
floating action button is spliced in as content and appears once per slice — twice or more
down the finished page. That is a property of the screen, not a bad stitch. Say so in the
report rather than re-running.

A stronger check when a stitch looks suspect: the output height should be about the first
slice plus the sum of the scroll steps. If it is far short, content was dropped no matter
what `all_spliced` says.

## 5. Clean Up

Delete the slice directory. The stitched PNG is the only artifact that survives. Name it for what it shows — `settings.png`, `search-results.png`, `product-detail.png` — never `screenshot-1.png` or a timestamp.

## Tests

`scripts/test_stitch_screens.py` builds synthetic pages and asserts the stitch reconstructs
them row for row: a vertical page with fixed chrome on both edges, including a pinned header
carrying a live-updating value; the same page rotated, for the horizontal axis; a page whose
large navigation title collapses after the first slice; and a still screen holding one
sideways-scrolling carousel, which must be found and cropped to. Run it after any change to
the stitcher.

## Reporting

State the target, the output path, the number of slices, whether the capture was truncated by infinite scroll, and every seam's status. If any seam was butt-joined, say so plainly instead of presenting the image as complete.
