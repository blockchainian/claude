# Handoff: simulator support for `ios-take-screenshot`

Date: 2026-09-06. Repo: `github.com/blockchainian/claude`, branch `main`.
Plugin: `plugins/build-ios-apps`, version 0.5.3 at handoff.

## Status: nothing on the simulator has been run

The skill does not work on a simulator today, and this is stronger than "unverified". Both
helper scripts read `devicectl`, which does not see simulators, so discovery and app lookup
fail at the first step. Every capture technique in the skill is Appium-specific and targets
a physical device.

No simulator was booted, no app was installed on one, and no simulator screen was captured
or stitched during the work that produced this handoff. The only simulator knowledge here
comes from reading two XcodeBuildMCP tool schemas — see item 4 below. Treat every simulator
statement in this document as a design note to be tested, not as a finding.

The real-device path, by contrast, was cold-run end to end from a clean machine.

## The decision, already made

Add simulator capture to the **existing** `ios-take-screenshot` skill, branching on target.
Do not create a second skill.

Reason: the divergence is one layer. The stitcher, output convention, slice discipline,
settle rule, endless-list rule, cleanup and reporting are all shared. Only app lookup,
session setup, and the scroll/screenshot calls differ. Two skills would force the caller to
pick correctly every time, and `ios-take-screenshot` is the generic name — it wins whenever
the target is unstated, which is most of the time.

If the branches later contradict rather than merely differ, the fallback shape is two thin
skills over a shared `scripts/` directory. There is precedent: the profiling skills already
build on `ios-debugger-agent`.

## What exists and is verified (real device only)

Real-device capture works end to end and was cold-run from a clean machine (no preset, no
`~/.appium`, no cached WebDriverAgent, none installed on the phone).

- `SKILL.md` — the workflow, real devices only, states that explicitly
- `scripts/discover_ios_setup.py` — reads UDID, team id, WDA bundle id and signed IPA off
  the machine; prints `suggestedCapabilities`; exit 1 with `missing` when not ready
- `scripts/find_ios_app.sh` — bundle id lookup; passes `--include-all-apps`, without which
  `devicectl` hides App Store apps
- `scripts/stitch_screens.py` — the stitcher; `--axis vertical|horizontal`
- `scripts/test_stitch_screens.py` — synthetic regression, exact reconstruction, both axes

Captures produced and checked: a token page (4 slices, 5401px), a live feed, a search
screen, a non-scrolling profile screen, a finite About tab, and a horizontal carousel
(6 slices, 4499x354).

## The work

1. **App lookup** — `find_ios_app.sh` is `devicectl`-only. Add a simulator path via
   `xcrun simctl listapps <udid>`, or a sibling script. Keep the JSON output shape.
2. **Setup** — `discover_ios_setup.py` is device-only and its WebDriverAgent logic is
   meaningless for a simulator. A simulator needs a booted device and an installed app,
   nothing more. Decide whether to branch inside it or add `discover_ios_simulator.py`.
3. **Drive layer** — replace Appium calls with XcodeBuildMCP for the simulator branch.
4. **The one real asymmetry**, read from the tool schemas — not executed, so confirm it
   before designing around it:
   - `xcodebuildmcp swipe` takes `withinElementRef` → element-scoped scrolling exists
   - `xcodebuildmcp screenshot` takes only `returnFormat` → **cannot capture one element**

   So a sideways region must be captured full screen and cropped to the element's rect from
   `snapshot_ui` before stitching. Full-screen slices of a scrolling band splice in the
   wrong place, because the static remainder matches at any offset. A crop helper is
   probably the cleanest addition.
5. **Description** — currently says "Not for the simulator". Update when it is true.
6. **README** — the Skills table has a Target column; update that row.

## Facts that cost time to learn

Appium, real device:

- `direction` is the direction the **content** moves. `direction=up` goes further **down**
  the page. Getting this backwards makes every overlap measurement read as "nothing moved".
- Scrolling must be scoped to a container's `elementUUID`; a bare gesture often moves
  nothing, and custom `x/y/endX/endY` drags frequently move nothing at all.
- `**/XCUIElementTypeScrollView` returns the **first** match, which may be the wrong one.
  Find the right one by shape: read `rect` via `appium_get_element_attribute`. A horizontal
  scroller is wide and short (e.g. 393x118); a page container is nearly screen height
  (393x704). Walking indices and scrolling to see what moves wastes turns and produced two
  wrong conclusions.
- Element-scoped screenshots crop to the element and are what make horizontal capture sound.
- An element's rect rounds between frames, so captures can differ by a pixel. The stitcher
  trims to the common size; do not reintroduce a hard size check.
- A live feed never settles. Bound the settle wait; a transition decays to near zero within
  about a second, live content holds a steady difference indefinitely.
- Two device switches cannot be set from the Mac, and one cannot even be read: Developer
  Mode, and Settings → Developer → Enable UI Automation. Without the second, session
  creation fails as a bare `xcodebuild failed with code 65`.

Stitcher internals, in case it needs changing:

- Fixed chrome is found by the **share of pixels in a row that change**, not the average
  change, so a pinned header showing a live value is still recognised as chrome.
- Each slice must advance the image by a minimum fraction. Without that floor the matcher
  aligns one slice's pinned header against the previous one's and collapses the stitch to a
  single screen.
- Full-band matches are preferred; partial overlap is a fallback only. Letting both compete
  on raw error let a lucky 60-row match beat the true 200-row one and moved a seam by 700px.
- Horizontal support is a transpose in and out; the matching code is unchanged.

Packaging, the trap that hid a day of work:

- Claude Code loads the plugin from `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/`,
  **not** from the marketplace repo. Editing the repo changes nothing that runs.
- `claude plugin install` says "already installed" and does nothing. Use
  `claude plugin update <plugin>@<marketplace>`, which needs the version in `plugin.json`
  to have moved. Then restart.
- Symlinking the cache directory to the repo makes edits live instantly and is good for
  iteration, but it hides packaging bugs — a missing MCP server was invisible until a real
  install was tested. Do a genuine install before believing anything works.
- The remote needs the `blockchainian` SSH alias (`git@blockchainian:...`); plain
  `git@github.com:` authenticates as the wrong account and the push is denied.

## Verification bar

Match what the device path went through, or the work is not done:

1. `scripts/test_stitch_screens.py` passes.
2. A cold run from a clean state, through a **real install** rather than a symlink: the
   skill loads from the installed copy, discovery reports what is missing, setup completes,
   a screen is captured, stitched, and the slices are deleted.
3. Exercise all four shapes, since each broke something: a finite scrolling screen, a
   non-scrolling screen, an endless list, and a horizontally scrolling region.
4. Open the stitched output and check continuity. On a dark UI a flat band matches anywhere,
   so a low seam error is not proof — ordered content that stays ordered is.

## Do not

- Do not make the skill app-specific. It described one app's carousel once and had to be
  rewritten.
- Do not claim a capability before it is exercised. The simulator claim shipped for hours
  before anything supported it.
- Do not tap coordinates without a fresh screenshot of what is under them. A remembered
  tab-bar position opened a payment sheet on a real account.
- Do not present a truncated capture as complete. An endless list stops at two screens, and
  the report must say so.
