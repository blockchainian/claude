---
name: check-mobile-design
description: Check a built iOS screen against a design reference and report how far off it is, as measured JSON plus a composite PNG. Use when implementing an iOS screen to match a design pixel-close, iterating an "actual vs desired" loop, or asking why a built screen does not match a reference. Attributes each difference to a named view when given the app's frames from snapshot_ui.
---

# Check Mobile Design

Drive the "build this screen from a reference" loop: compare the app you are
building (**actual**) against a given design (**desired**), find what differs,
fix, repeat. The script reports two things — an image diff for *where and how
much*, and, when you hand it the app's on-screen elements, *what* each diff
falls on ("the `Buy` button colour is off", not "region (12,340) differs").

The target is **perceptual, tolerance-bounded** equivalence, not literal pixel
equality. Device scale, the status bar, antialiasing and colour profiles make
identical designs differ by a few units; chasing zero never converges. The
`pass` field is the loop's stop condition.

## The loop

1. **Capture the actual screen.** Use `ios-take-screenshot` (or, for a single
   viewport, XcodeBuildMCP `screenshot`). Save the desired reference alongside.
2. **Lift the view frames (the hybrid step, optional but recommended).** Call
   `mcp__plugin_mobile_xcodebuildmcp__snapshot_ui` on the running app. Convert
   its tree to `elements.json`: a JSON **list** of
   `{"label": str, "type": str, "bbox": [x, y, w, h]}`, with `bbox` in
   **actual-image pixels**. `snapshot_ui` frames are in points, so multiply by
   the device scale (`screenshot_width_px / root_frame_width_pt`, e.g. ×2 or
   ×3). Keep the leaf views that render something (labels, buttons, images);
   drop full-screen containers.
3. **Run the diff:**

   ```
   uv run "${CLAUDE_PLUGIN_ROOT}/skills/check-mobile-design/scripts/check_design.py" \
     --actual actual.png --desired desired.png --out-dir diff \
     [--hierarchy elements.json] \
     [--mask-top 47 --mask-bottom 34] \
     [--tol-de 3 --tol-px 2 --region-area-pct 0.5]
   ```

4. **Read `diff/diff.json`.** If `pass` is true, stop. Otherwise decide the next
   edit from the failing entries, then look at `diff/diff.png` (actual |
   desired | heatmap) to confirm.
5. Re-capture and repeat.

## Masking the chrome

The status bar (clock, battery) and home indicator differ between any two
captures and are not design bugs — exclude them with `--mask-top` /
`--mask-bottom`, in **pixels**. Typical modern iPhone at ×3: `--mask-top 141`
(≈47pt status bar) and `--mask-bottom 102` (≈34pt home indicator); at ×2 use
≈94 and ≈68. Anything in a masked band is left out of the regions, the summary,
and `pass`.

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
