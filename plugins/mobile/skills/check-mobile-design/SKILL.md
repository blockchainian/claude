---
name: check-mobile-design
description: Check a built iOS screen against a design reference and report how far off it is, as measured JSON plus a composite PNG. Use when implementing an iOS screen to match a design pixel-close, iterating an "actual vs desired" loop, or asking why a built screen does not match a reference. Attributes each difference to a named view when given the app's frames from snapshot_ui.
---

# Check mobile design

This skill drives the "build this screen from a reference" loop: compare the app you are building
(**actual**) against the given design (**desired**), fix what differs, and repeat. The image diff
says where and how much the screens differ; when you also pass the app's on-screen views, the
report names what each difference falls on ("the `Buy` button colour is off", not "region
(12,340) differs"). The goal is equivalence within tolerance, not identical pixels: device scale,
the status bar, antialiasing and colour profiles always leave a few units of difference, so
chasing zero never converges. Stop when the report's `pass` is true.

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call, not the
app repository's working directory or a host plugin variable. If the loaded path is unavailable,
stop and report it before running the script.

```bash
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Setup

The script needs `uv` on PATH; it declares its own pillow and numpy dependencies. Take
`snapshot_ui` and `screenshot` from the `xcodebuildmcp` server under the names the host's tool
inventory or tool search actually lists, since namespace prefixes differ between hosts. If either
is missing, report it rather than guessing a name.

## Capture the screens

Capture the actual screen with `ios-take-screenshot`, or with `screenshot` for a single viewport,
and save the desired reference beside it. Both must be PNGs of the same screen; a desired image of
a different size is resized to the actual one, and `normalize.resized` in the report says so.

## Lift the view frames

This step is optional but recommended, because without it every difference comes back unnamed.
Call `snapshot_ui` on the running app and convert its tree into `elements.json`, a JSON list of
`{"label": str, "type": str, "bbox": [x, y, w, h]}` with `bbox` in actual-image pixels.
`snapshot_ui` frames are in points, so multiply them by the device scale,
`screenshot_width_px / root_frame_width_pt` (for example ×2 or ×3). Keep the leaf views that render
something, such as labels, buttons and images, and drop full-screen containers.

## Run the diff

```bash
"$SKILL_DIR/scripts/check_design.py" \
  --actual actual.png --desired desired.png --out-dir diff \
  [--hierarchy elements.json] \
  [--mask-top 47 --mask-bottom 34] \
  [--tol-de 3 --tol-px 2 --region-area-pct 0.5]
```

Read `diff/diff.json`. If `pass` is true, stop. Otherwise choose the next edit from the failing
entries, confirm it against `diff/diff.png` (actual | desired | heatmap), then re-capture and run
again.

The status bar (clock, battery) and home indicator differ between any two captures and are not
design bugs, so exclude them with `--mask-top` and `--mask-bottom`, given in pixels. On a typical
modern iPhone at ×3 use `--mask-top 141` (the ≈47 pt status bar) and `--mask-bottom 102` (the
≈34 pt home indicator); at ×2 use about 94 and 68. Masked bands are left out of the regions, the
summary and `pass`.

## Read the report

`pass` is true when every attributed element is within tolerance and no unattributed region
exceeds `regionAreaPct` of the frame.

`elements[]` has one entry per view you supplied, with `deltaE` (perceptual colour distance, CIELAB
CIE76), `colorActual` and `colorDesired` as hex, `offsetPx`, and a `kind` that tells you what to fix:

- `color`: right place, wrong colour. Fix the fill, tint or text colour.
- `position`: right look, shifted by `offsetPx` `[dx, dy]`. Fix the frame, padding or constraints.
- `missing`: the design has content here and the build is blank. Add the view.
- `extra`: the build has content here and the design is blank. Remove it.
- `ok`: within tolerance.

`regions[]` lists differences not covered by any element, such as gradients, shadows and images,
each as `{bbox, areaPct, deltaE}`, largest and worst first. A difference that lands in `regions`
instead of `elements` means `elements.json` lacks that view; add it to get a named result.
`summary` holds `meanDeltaE`, `maxDeltaE`, `diffAreaPct` and the counts.

## Tolerances

The defaults are `--tol-de 3` for colour, `--tol-px 2` for offset and `--region-area-pct 0.5`.
Loosen `--tol-de` toward 5 when the reference is a lossy JPEG or uses a different colour profile,
and tighten it toward 1 for flat-colour UI. An element shifted by more than about 8 px is reported
as a `color` or `missing` mismatch rather than a `position` shift, so get the coarse placement
right first and then re-run.
