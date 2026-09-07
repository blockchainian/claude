# Simulator support for `ios-take-screenshot`

Date: 2026-09-06. Repo: `github.com/blockchainian/claude`, branch `main`.
Plugin: `plugins/build-ios-apps`, 0.5.3 at the start of this work, 0.6.2 at the end.

## Status: shipped and exercised on a simulator

The skill captures a simulator screen end to end. It was run against a real app on a
booted iPhone 16, through a genuine `claude plugin update` install rather than a symlink,
across all four screen shapes that the device path had to handle.

The design decision from the first draft held: one skill, branching on target. The
divergence really is one layer. What that draft got *wrong* was which layer — see below.

## What the first draft predicted, and what is actually true

The original plan was written from reading two tool schemas. Two of its three load-bearing
claims did not survive contact.

| Planned | Actual |
|---|---|
| Capture with the XcodeBuildMCP `screenshot` tool | It returns a **369x800 JPEG** for a 1179x2556 screen — downscaled and lossy whatever the file is named. Slices come from `xcrun simctl io ... screenshot --type=png` instead: native resolution, ~0.1s, writes straight into the slice directory with no copy step. |
| Crop a sideways region to its element rect from `snapshot_ui` | **`snapshot_ui` reports no geometry at all** — no rect, no frame, no coordinates. Only `ref\|action\|role\|label\|identifier`. The plan was impossible as written. |
| `swipe` takes `withinElementRef`, so element-scoped scrolling exists | True, and the only prediction that held. |

The replacement for the rect: `--crop-band` on the stitcher finds the region in the pixels.
A band that scrolls slides its whole content past the window, so its rows change; the rest
of the screen does not. That is the same measurement chrome detection already made, used
the other way round.

## What shipped

- `find_ios_app.sh --simulator <udid|booted>` — `simctl listapps` piped through `plutil`,
  same JSON shape as the device path. `devicectl` cannot see simulators at all.
- `discover_ios_setup.py --target simulator` — booted simulators and a `sessionDefaults`
  object. No WebDriverAgent, no provisioning profile, no session to create.
- `stitch_screens.py --crop-band` — finds the moving band and crops to it.
- `SKILL.md` — branches on target in three places: app lookup, setup, and the calls that
  scroll and capture. Everything downstream of a slice is shared.
- README target column, plugin and marketplace descriptions.

## Bugs found while verifying, all pre-existing or introduced and fixed here

Each has a regression test that fails without its fix.

1. **The first slice's expanded navigation title broke chrome detection.** iOS draws a large
   title at scroll offset zero and collapses it once the page moves, so that band differs
   between slice 1 and every later slice and was measured as content. Detection stopped at
   the status bar and left a strip of title bar inside every content region, which the
   matcher aligned against itself. A six-slice capture came out 4862px against a true
   6131px. **This was not simulator-specific — it was the shared stitcher, so the shipped
   device path had it too.** Fixed by measuring chrome on the slices the crop applies to.
2. **A band was detected too tightly.** A row of text is mostly background, so few of its
   pixels move; a real tab strip cropped to 35px with the glyphs cut in half. Fixed with
   two thresholds — a strict one to find the band, a loose one to grow it to its edges.
3. **The longest moving span is not the band.** On a live screen a token list whose prices
   and sparklines redraw spans far more rows than a tab strip, and `--crop-band` locked onto
   it. Fixed by ranking candidates on how completely each changes rather than how tall it
   is, with a floor so a blinking detail cannot win.
4. **A default-distance swipe advances roughly a whole viewport**, leaving nothing to match
   on. On one capture it skipped a short holdings section entirely: two slices with no
   overlap and a stitched page missing a row while looking perfectly plausible. The skill
   now says to scroll about half a screen, and that number is measured, not guessed: at
   `distance` 0.5 the same page advanced 1172, 1176 and 1175px of a 2020px content window —
   58% a step, 42% overlap — and the row that had gone missing came back.

## Two traps worth keeping

**`all_spliced: true` is not proof, and `--max-error` is never the fix.** Raising the
threshold does not find a better alignment, it accepts a worse one. On the capture in bug 1
it made every seam report success and produced an image a third shorter than the page, with
the missing rows gone silently. Twice in this work a confident diagnosis of "it is just the
threshold" was wrong, both times because the diagnosis was verified against the tool's own
flag. Measure the true scroll offsets independently instead.

**A height check catches a bad splice but not a bad butt join.** A butt join pads in a whole
untrimmed slice, so the output reads *longer* than the arithmetic predicts even while a
section is missing. That is how bug 4 hid. For a butt-joined seam, read the bottom of the
earlier slice against the top of the next one and ask what should sit between them.

## Verification

1. `scripts/test_stitch_screens.py` passes: a vertical page with a pinned header carrying a
   live value, the same page transposed, a collapsing large title, and a carousel band with
   sparse edges beside a taller live-updating decoy.
2. Cold run through a real `claude plugin update` install, not a symlink. Both MCP servers
   present in the installed `.mcp.json`; scripts and tests run from the installed copy.
3. All four shapes captured against ChadWallet on the simulator: an endless list (truncated
   at two screens, as the rule requires), a horizontally scrolling tab strip, a finite
   scrolling page, and a non-scrolling screen passed through as a single slice.
   The finite page was then recaptured at `distance` 0.5 to test the scroll guidance: five
   slices, every seam spliced, 6145px, and 2307 + 1172 + 1176 + 1175 + 315 = 6145 exactly.
4. Every stitched output opened and checked for continuity.
5. The document followed **unaided** — an agent given no UDID, no paths and no bundle id,
   told to derive each from the skill's own steps and to stop rather than invent. It
   produced a correct 6145px capture from one slice directory. Every run before that was
   steered by paths supplied in the brief, which had been masking a real bug: the setup
   block read as though it reused a slice directory, and in a fresh shell minted a new one
   per command. Two orphaned directories holding a single slice each were still on disk
   from earlier runs.

That last run also found the document turning on a number — "a mean absolute pixel
difference below about 2" — that it never said how to compute, on a machine whose system
python has no imaging library. `frame_diff.py` now answers it, and the harder question
underneath: a screen carrying a net worth or a claimable balance never reaches zero
difference, so a raw difference reads as movement forever. It reports the offset at which
the later frame's content sits in the earlier one — zero when only values redrew.

## Known limits, documented in the skill

- **Two slices cannot self-diagnose their chrome.** Detection compares slices, so with a
  single pair the first slice's expanded title is half the evidence. An endless-list capture
  is exactly two slices. The seam fails loudly rather than quietly; pass `--sticky-top`.
- **A floating button over the middle of the page repeats once per slice.** Chrome is
  detected at the edges only. It is a property of the screen, not a bad stitch.
- **A live feed is a composite**, not a snapshot of one instant. Unchanged from the device
  path, and it showed up again here: a token list's prices differ between slices.

## Facts about driving a simulator

- `elementRef`s go stale as soon as the screen moves; a stale one fails
  `TARGET_NOT_ACTIONABLE`. Each `swipe` response carries a fresh snapshot — take the next
  ref from there. A top-level container sometimes keeps its ref string and a nested one does
  not, so never assume.
- `direction=up` advances further down the page, same as Appium.
- No settle delay is needed: by the time `swipe` returns the pixels have stopped.
- Expect `SNAPSHOT_CAPTURE_FAILED` on most swipes. It is the accessibility tree timing out,
  not the rendering.
- Some rows are exposed as text rather than buttons and cannot be tapped by ref at all —
  stock Settings below its first level, for one.

## Packaging

Unchanged and still true: Claude Code loads the plugin from
`~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/`, not from the marketplace repo.
`claude plugin install` says "already installed" and does nothing; use
`claude plugin update <plugin>@<marketplace>` with a moved version in `plugin.json`, then
restart. It reads the local marketplace clone, so an unpushed commit still installs. The
remote needs the `blockchainian` SSH alias.

## Open

- Nothing pushed. The branch is `main`, several commits ahead of origin.
- The device path has not been re-run since the chrome-detection fix. The fix is covered by
  a regression test and makes detection strictly better informed, but it changes shared code
  and no phone capture has exercised it.
