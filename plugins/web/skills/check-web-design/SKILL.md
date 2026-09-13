---
name: check-web-design
description: Check a built web page against a design reference and report how far off it is, as measured JSON plus a composite PNG. Use when implementing a web page to match a design pixel-close, iterating an "actual vs desired" loop, or asking why a built page does not match a reference. Attributes each difference to a named element when given the page's DOM rects.
---

# Check Web Design

Drive the "build this page from a reference" loop: compare the page you are
building (**actual**) against a given design (**desired**), find what differs,
fix, repeat. The script reports two things — an image diff for *where and how
much*, and, when you hand it the page's elements, *what* each diff falls on
("the `Buy` button colour is off", not "region (12,340) differs").

The target is **perceptual, tolerance-bounded** equivalence, not literal pixel
equality. Device pixel ratio, browser zoom, antialiasing, font rendering and
colour profiles make identical designs differ by a few units; chasing zero never
converges. The `pass` field is the loop's stop condition.

## The loop

1. **Capture the actual page.** Take a browser screenshot of the built page —
   `claude-in-chrome`, or a DevTools/Playwright full-page capture when the design
   runs below the fold. Save the design mock as `desired.png`.
2. **Build `elements.json` (the hybrid step, optional but recommended)** — a JSON
   **list** of `{"label": str, "type": str, "bbox": [x, y, w, h]}`, with `bbox`
   in **actual-image pixels**. This turns anonymous regions into named elements.
   Read it from the DOM in the page (e.g. `claude-in-chrome` `javascript_tool`):

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
3. **Run the diff:**

   ```
   uv run "${CLAUDE_PLUGIN_ROOT}/skills/check-web-design/scripts/check_design.py" \
     --actual actual.png --desired desired.png --out-dir diff \
     [--hierarchy elements.json] \
     [--mask-top 0 --mask-bottom 0] \
     [--tol-de 3 --tol-px 2 --region-area-pct 0.5]
   ```

4. **Read `diff/diff.json`.** If `pass` is true, stop. Otherwise decide the next
   edit from the failing entries, then look at `diff/diff.png` (actual |
   desired | heatmap) to confirm.
5. Re-capture and repeat.

## Reading `diff.json`

- `pass` — true when every attributed element is within tolerance and no
  unattributed region exceeds `regionAreaPct` of the frame.
- `elements[]` — one per element you supplied: `deltaE` (perceptual colour
  distance, CIELAB CIE76), `colorActual` / `colorDesired` (hex), `offsetPx`,
  and `kind`:
  - `color` — right place, wrong colour. Fix the fill/background/text colour.
  - `position` — right look, shifted by `offsetPx` `[dx, dy]`. Fix the layout,
    margin, or padding.
  - `missing` — the design has content here, the build is blank. Add the element.
  - `extra` — the build has content here, the design is blank. Remove it.
  - `ok` — within tolerance.
- `regions[]` — differences not covered by any element (gradients, shadows,
  images), each `{bbox, areaPct, deltaE}`, largest-and-worst first. If diffs
  land in `regions` rather than `elements`, your `elements.json` is missing that
  element — add it for a named result.
- `summary` — `meanDeltaE`, `maxDeltaE`, `diffAreaPct`, and counts.

## Masking

Web pages usually have no chrome to mask — leave `--mask-top` / `--mask-bottom`
at 0. Mask a sticky header or a cookie banner the design omits, in **pixels**,
if one is on screen; anything in a masked band is left out of the regions, the
summary, and `pass`.

## Tolerances

`--tol-de` (colour, default 3), `--tol-px` (offset, default 2),
`--region-area-pct` (default 0.5). Loosen `--tol-de` toward 5 when the reference
is a lossy JPEG or a different colour profile; tighten toward 1 for flat-colour
UI. Offsets larger than ~8px are reported as a colour/`missing` mismatch rather
than a shift — re-run after the coarse placement is right.

## Requirements

- `uv` on PATH (the script declares its own pillow + numpy deps).
- Both inputs are PNGs of the same page; different sizes are resized to the
  actual before comparison (`normalize.resized` reports it).
