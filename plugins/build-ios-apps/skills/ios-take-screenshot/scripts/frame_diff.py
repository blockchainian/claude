#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
"""Compare two screenshots: how much they differ, and whether the page scrolled.

The capture loop turns on one question after every scroll — did the page move? —
and a raw difference cannot answer it on a screen that carries a live value. A
price that ticks between frames registers a difference while nothing scrolled,
and a short scroll near the bottom of a page registers about as much.

So this reports both: the mean absolute difference, and the offset at which the
later frame's content is actually found in the earlier one. A page that scrolled
has a non-zero offset with a low error at that offset. A page that only flickered
matches best at offset 0.

Fixed chrome must be left out of that search: a pinned header sits at the same
rows in both frames, so it matches at offset zero and outweighs the content
below it. Unless its height is given, it is detected from the pair the same way
the stitcher detects it.

Prints JSON.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

from stitch_screens import detect_sticky

BAND = 400          # rows of the later frame looked for in the earlier one
MIN_OVERLAP = 100   # rows the band must still share with the earlier frame at the far end
MOVED_DIFF = 2.0    # mean absolute difference that counts as "these differ"


def gray(path: Path) -> np.ndarray:
    return np.asarray(Image.open(path).convert("L"), dtype=np.int16)


def crop(a: np.ndarray, top: int, bottom: int) -> np.ndarray:
    return a[top:a.shape[0] - bottom] if bottom else a[top:]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("before", type=Path)
    ap.add_argument("after", type=Path)
    ap.add_argument("--sticky-top", type=int, default=None,
                    help="pixels of fixed chrome to ignore at the top (default: detected)")
    ap.add_argument("--sticky-bottom", type=int, default=None,
                    help="pixels of fixed chrome to ignore at the bottom (default: detected)")
    args = ap.parse_args()

    for p in (args.before, args.after):
        if not p.is_file():
            sys.exit(f"missing image: {p}")

    a, b = gray(args.before), gray(args.after)
    if a.shape != b.shape:
        sys.exit(f"frames differ in size: {args.before.name} is {a.shape[1]}x{a.shape[0]}, "
                 f"{args.after.name} is {b.shape[1]}x{b.shape[0]}. Slices of one screen "
                 f"are the same size; these came from different simulators or captures.")
    h = a.shape[0]

    # Chrome is what holds still while content moves; when nothing moves at
    # all there is none to find, and the detector would report its cap.
    auto_top, auto_bottom = (0, 0) if float(np.abs(a - b).mean()) < MOVED_DIFF \
        else detect_sticky([a, b])
    top = auto_top if args.sticky_top is None else args.sticky_top
    bottom = auto_bottom if args.sticky_bottom is None else args.sticky_bottom
    a, b = crop(a, top, bottom), crop(b, top, bottom)
    h = a.shape[0]

    diff = float(np.abs(a - b).mean())

    # A long scroll leaves only part of the band inside the earlier frame, so
    # the band shrinks toward the far end rather than the search stopping short.
    band_h = min(BAND, h // 2)
    band = b[:band_h]
    best_off, best_err = 0, float("inf")
    for off in range(max(1, h - MIN_OVERLAP)):
        n = min(band_h, h - off)
        err = float(np.abs(a[off:off + n] - band[:n]).mean())
        if err < best_err:
            best_off, best_err = off, err

    print(json.dumps({
        "mean_abs_diff": round(diff, 2),
        "differs": diff > MOVED_DIFF,
        "scrolled_px": best_off,
        "match_error": round(best_err, 2),
        "scrolled": best_off > 0 and best_err < diff,
        "content_height": h,
        "sticky_top": top,
        "sticky_bottom": bottom,
        "scrolled_fraction": round(best_off / h, 3) if h else 0.0,
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
