---
name: check-web-design
description: Check a built web page against a design reference and report how far off it is, as measured JSON plus a composite PNG. Use when implementing a web page to match a design pixel-close, iterating an "actual vs desired" loop, or asking why a built page does not match a reference. Attributes each difference to a named element when given the page's DOM rects.
---

# Check web design

This skill compares the page you are building (the actual) with a design (the desired) so you can fix what differs and repeat. The image diff says where and how much the two differ; when you also pass the page's elements, each difference is named after the element it falls on ("the `Buy` button colour is off", not "region (12,340) differs"). Aim for equivalence within tolerance, not identical pixels: device pixel ratio, browser zoom, antialiasing, font rendering and colour profiles make identical designs differ by a few units, so chasing zero never converges. Stop when `pass` is true.

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call, because shell variables may not persist between calls. Use the installed path, not the caller's working directory or a host-specific plugin variable; if the loaded path is unavailable, stop and report it before running the script.

```bash
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Setup

The script needs `uv` on PATH; it declares its own pillow and numpy dependencies. Take screenshots and read the DOM with the host's connected browser tools (for example Claude's `claude-in-chrome` or Codex's connected Chrome tools). Check that the tool supports the capture or DOM operation you need, and report a missing capability rather than assuming a tool name exists.

## Compare a page with its design

Take a screenshot of the built page as `actual.png`, using the connected browser tools, or a DevTools or Playwright full-page capture when the design runs below the fold. Save the design mock as `desired.png`. Both must be PNGs of the same page; a desired image of a different size is resized to the actual one, and `normalize.resized` reports it.

Next, build `elements.json`. It is optional but recommended, because it turns anonymous regions into named elements. It is a JSON list of `{"label": str, "type": str, "bbox": [x, y, w, h]}` with `bbox` in actual-image pixels. Read it from the page with the host's DOM evaluation tool:

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

Here `s` rescales CSS pixels to screenshot pixels, absorbing `devicePixelRatio` and any capture scaling. Keep `scrollX` and `scrollY` for a full-page screenshot and drop them for a viewport-only one. Set `SELECTORS` to the elements you care about, such as buttons, headings and images, not every node.

Then run the diff:

```
"$SKILL_DIR/scripts/check_design.py" \
  --actual actual.png --desired desired.png --out-dir diff \
  [--hierarchy elements.json] \
  [--mask-top 0 --mask-bottom 0] \
  [--tol-de 3 --tol-px 2 --region-area-pct 0.5]
```

Read `diff/diff.json`. If `pass` is true, stop. Otherwise choose the next edit from the failing entries, confirm it in `diff/diff.png` (actual, desired and heatmap side by side), then re-capture and run again.

## Reading the report

`pass` is true when every supplied element is within tolerance and no unattributed region covers more than `regionAreaPct` of the frame. `summary` holds `meanDeltaE`, `maxDeltaE`, `diffAreaPct` and counts.

`elements[]` has one entry per element you supplied, with `deltaE` (perceptual colour distance, CIELAB CIE76), `colorActual` and `colorDesired` as hex, `offsetPx`, and a `kind` that says what to fix:

- `color`: right place, wrong colour. Fix the fill, background or text colour.
- `position`: right look, shifted by `offsetPx` `[dx, dy]`. Fix the layout, margin or padding.
- `missing`: the design has content here and the build is blank. Add the element.
- `extra`: the build has content here and the design is blank. Remove it.
- `ok`: within tolerance.

`regions[]` lists differences no element covers, such as gradients, shadows and images, each as `{bbox, areaPct, deltaE}`, largest and worst first. A difference that lands in `regions` rather than `elements` means `elements.json` lacks that element; add it to get a named result.

## Masking and tolerances

Web pages usually have no fixed chrome, so leave `--mask-top` and `--mask-bottom` at 0. If a sticky header or a cookie banner the design omits is on screen, mask it in pixels; anything in a masked band is left out of the regions, the summary and `pass`.

`--tol-de` sets the colour tolerance (default 3), `--tol-px` the offset tolerance (default 2) and `--region-area-pct` the largest unattributed region allowed (default 0.5). Loosen `--tol-de` toward 5 when the reference is a lossy JPEG or uses a different colour profile, and tighten it toward 1 for flat-colour UI. An offset larger than about 8px is reported as a colour or `missing` mismatch rather than a shift, so get the coarse placement right first and then re-run.

## Tests

```sh
npm run test:web
```
