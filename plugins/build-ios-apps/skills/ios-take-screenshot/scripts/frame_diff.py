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

Prints JSON.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

BAND = 400          # rows of the later frame looked for in the earlier one
MOVED_DIFF = 2.0    # mean absolute difference that counts as "these differ"


def gray(path: Path, top: int, bottom: int) -> np.ndarray:
    a = np.asarray(Image.open(path).convert("L"), dtype=np.int16)
    return a[top:a.shape[0] - bottom] if bottom else a[top:]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("before", type=Path)
    ap.add_argument("after", type=Path)
    ap.add_argument("--sticky-top", type=int, default=0,
                    help="pixels of fixed chrome to ignore at the top")
    ap.add_argument("--sticky-bottom", type=int, default=0,
                    help="pixels of fixed chrome to ignore at the bottom")
    args = ap.parse_args()

    for p in (args.before, args.after):
        if not p.is_file():
            sys.exit(f"missing image: {p}")

    a = gray(args.before, args.sticky_top, args.sticky_bottom)
    b = gray(args.after, args.sticky_top, args.sticky_bottom)
    h = min(a.shape[0], b.shape[0])
    w = min(a.shape[1], b.shape[1])
    a, b = a[:h, :w], b[:h, :w]

    diff = float(np.abs(a - b).mean())

    band_h = min(BAND, h // 2)
    band = b[:band_h]
    best_off, best_err = 0, float("inf")
    for off in range(h - band_h):
        err = float(np.abs(a[off:off + band_h] - band).mean())
        if err < best_err:
            best_off, best_err = off, err

    print(json.dumps({
        "mean_abs_diff": round(diff, 2),
        "differs": diff > MOVED_DIFF,
        "scrolled_px": best_off,
        "match_error": round(best_err, 2),
        "scrolled": best_off > 0 and best_err < diff,
        "content_height": h,
        "scrolled_fraction": round(best_off / h, 3) if h else 0.0,
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
