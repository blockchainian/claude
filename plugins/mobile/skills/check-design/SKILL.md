---
name: check-design
description: Check a built screen against a design reference and report how far off it is, as measured JSON plus a composite PNG. Use when implementing a screen to match a design pixel-close, iterating an "actual vs desired" loop, or asking why a built screen does not match a reference. Attributes each difference to a named on-screen element when given the app's view frames.
---

# Check Design

Drive the "build this screen from a reference" loop: compare the app you are
building (**actual**) against a given design (**desired**), find what differs,
fix, repeat. The script reports two things — an image diff for *where and how
much*, and, when you hand it the app's on-screen elements, *what* each diff
falls on ("the `Buy` button colour is off", not "region (12,340) differs").

The target is **perceptual, tolerance-bounded** equivalence, not literal pixel
equality. Device scale, browser zoom, the status bar, antialiasing and
colour/font rendering make identical designs differ by a few units; chasing zero
never converges. The `pass` field is the loop's stop condition.

The engine is platform-neutral — two PNGs plus an optional element list. iOS and
web are just two ways to produce those inputs; see "Capturing the inputs".

## The loop

1. **Capture the actual screen** as a PNG and save the desired reference next to
   it (see "Capturing the inputs").
2. **Build `elements.json` (the hybrid step, optional but recommended)** — a JSON
   **list** of `{"label": str, "type": str, "bbox": [x, y, w, h]}`, with `bbox`
   in **actual-image pixels**. This is what turns anonymous regions into named
   elements. Keep the leaf elements that render something (text, buttons,
   images); drop full-screen containers. See "Capturing the inputs" for how to
   get the frames on each platform.
3. **Run the diff:**

   ```
   uv run "${CLAUDE_PLUGIN_ROOT}/skills/check-design/scripts/check_design.py" \
     --actual actual.png --desired desired.png --out-dir diff \
     [--hierarchy elements.json] \
     [--mask-top 47 --mask-bottom 34] \
     [--tol-de 3 --tol-px 2 --region-area-pct 0.5]
   ```

4. **Read `diff/diff.json`.** If `pass` is true, stop. Otherwise decide the next
   edit from the failing entries, then look at `diff/diff.png` (actual |
   desired | heatmap) to confirm.
5. Re-capture and repeat.

## Capturing the inputs

### iOS
- **actual.png**: `ios-take-screenshot` (whole screen, stitched), or
  `mcp__plugin_mobile_xcodebuildmcp__screenshot` (one viewport).
- **elements.json**: `mcp__plugin_mobile_xcodebuildmcp__snapshot_ui` on the
  running app, converting each view to `{label, type, bbox}`. Its frames are in
  points — multiply by the device scale
  (`screenshot_width_px / root_frame_width_pt`, e.g. ×2 or ×3).

### Web
- **actual.png**: a browser screenshot of the built page — `claude-in-chrome`,
  or a DevTools/Playwright full-page capture when the design runs below the fold.
  Save the design mock as `desired.png`.
- **elements.json**: read the DOM in the page. Run this in the browser (e.g.
  `claude-in-chrome` `javascript_tool`) and save the JSON it returns:

  ```js
  const s = W / document.documentElement.clientWidth;  // W = actual.png width, px
  JSON.stringify([...document.querySelectorAll(SELECTORS)].map(e => {
    const r = e.getBoundingClientRect();
    return {
      label: e.getAttribute('aria-label') || e.id
             || e.textContent.trim().slice(0, 40) || e.tagName.toLowerCase(),
      type: e.tagName.toLowerCase(),
      bbox: [Math.round((r.left + scrollX) * s), Math.round((r.top + scrollY) * s),
             Math.round(r.width * s), Math.round(r.height * s)],
    };
  }).filter(e => e.bbox[2] > 0 && e.bbox[3] > 0));
  ```

  `s` rescales CSS pixels to the screenshot's pixels (it absorbs
  `devicePixelRatio` and any capture scaling). Keep `scrollX`/`scrollY` for a
  full-page screenshot; drop them for a viewport-only one. Choose `SELECTORS`
  for the elements you care about (buttons, headings, images), not every node.

## Masking the chrome

Anything in a masked band is left out of the regions, the summary, and `pass`;
mask with `--mask-top` / `--mask-bottom`, in **pixels**.

- **iOS**: the status bar (clock, battery) and home indicator differ between any
  two captures and are not design bugs. Typical modern iPhone at ×3:
  `--mask-top 141` (≈47pt status bar), `--mask-bottom 102` (≈34pt home
  indicator); at ×2 use ≈94 and ≈68.
- **Web**: usually nothing to mask (leave both at 0). Mask a sticky header or a
  cookie banner that the design omits if one is on screen.

## Reading `diff.json`

- `pass` — true when every attributed element is within tolerance and no
  unattributed region exceeds `regionAreaPct` of the frame.
- `elements[]` — one per view you supplied: `deltaE` (perceptual colour
  distance, CIELAB CIE76), `colorActual` / `colorDesired` (hex), `offsetPx`,
  and `kind`:
  - `color` — right place, wrong colour. Fix the fill/tint/text colour.
  - `position` — right look, shifted by `offsetPx` `[dx, dy]`. Fix the frame,
    padding, or constraints.
  - `missing` — the design has content here, the build is blank. Add the view.
  - `extra` — the build has content here, the design is blank. Remove it.
  - `ok` — within tolerance.
- `regions[]` — differences not covered by any element (gradients, shadows,
  images), each `{bbox, areaPct, deltaE}`, largest-and-worst first. If diffs
  land in `regions` rather than `elements`, your `elements.json` is missing that
  view — add it for a named result.
- `summary` — `meanDeltaE`, `maxDeltaE`, `diffAreaPct`, and counts.

## Tolerances

`--tol-de` (colour, default 3), `--tol-px` (offset, default 2),
`--region-area-pct` (default 0.5). Loosen `--tol-de` toward 5 when the reference
is a lossy JPEG or a different colour profile; tighten toward 1 for flat-colour
UI. Offsets larger than ~8px are reported as a colour/`missing` mismatch rather
than a shift — re-run after the coarse placement is right.

## Requirements

- `uv` on PATH (the script declares its own pillow + numpy deps).
- Both inputs are PNGs of the same screen; different sizes are resized to the
  actual before comparison (`normalize.resized` reports it).
