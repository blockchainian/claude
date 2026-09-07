#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
"""Stitch overlapping iOS screen slices into one full-screen PNG.

iOS screenshots capture only the visible viewport. Given slices taken while
scrolling down a screen, this removes the chrome that stays fixed on screen,
finds where each slice overlaps the previous one, and splices them so repeated
content appears exactly once.

Prints a JSON verdict. Every seam must report "spliced": true.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

PIXEL_DELTA = 20        # per-pixel difference that counts as "this pixel changed"
STATIC_ROW_FRAC = 0.15  # a row is fixed chrome if fewer than this share of pixels change
MAX_STICKY_FRAC = 0.30  # refuse to call more than this share of the screen chrome
DEFAULT_BAND = 200      # height of the band used to match one slice against the previous
DEFAULT_MAX_ERROR = 11.0
DEFAULT_SEARCH = 2400   # how far back up the accumulated image to look for the overlap
MIN_BAND_STD = 12.0     # a flatter band (empty dark background) matches anywhere
MIN_ADVANCE_FRAC = 0.2  # each slice must extend the image by this share of a screen
MIN_MATCH_ROWS = 60     # fewest overlapping rows that still make an alignment credible


def load(paths: list[Path], horizontal: bool = False) -> tuple[list[np.ndarray], list[np.ndarray]]:
    """Load slices, rotating them when the scroll axis is horizontal.

    All the matching below works on rows. Transposing a horizontally scrolled
    screen turns its columns into rows, so the same logic applies unchanged and
    the result is transposed back on the way out.
    """
    images = [Image.open(p) for p in paths]
    sizes = {im.size for im in images}
    if len(sizes) > 1:
        # Element-scoped captures can differ by a pixel as the element's rect
        # rounds between frames. Trim to the common size rather than refusing
        # a capture that is otherwise fine.
        w = min(im.size[0] for im in images)
        h = min(im.size[1] for im in images)
        if max(im.size[0] for im in images) - w > 4 or max(im.size[1] for im in images) - h > 4:
            sys.exit(f"slice sizes differ by more than rounding: {sorted(sizes)}")
        images = [im.crop((0, 0, w, h)) for im in images]

    rgb, gray = [], []
    for im in images:
        r = np.asarray(im.convert("RGB"))
        g = np.asarray(im.convert("L"), dtype=np.int16)
        if horizontal:
            r, g = r.transpose(1, 0, 2), g.transpose(1, 0)
        rgb.append(r)
        gray.append(g)
    return rgb, gray


def detect_sticky(gray: list[np.ndarray]) -> tuple[int, int]:
    """Find the chrome that stays fixed on screen while the content scrolls.

    Measured as the share of pixels in a row that change between slices, not the
    average change. A pinned header showing a live value — a price, a timer —
    changes a few digits while the rest of the row holds still, so its average
    difference is large but its changed-pixel share stays small. Content rows
    scroll wholesale and change nearly every pixel.
    """
    if len(gray) < 2:
        return 0, 0
    height = gray[0].shape[0]
    static = np.ones(height, dtype=bool)
    for a, b in zip(gray, gray[1:]):
        changed = (np.abs(a - b) > PIXEL_DELTA).mean(axis=1)
        static &= changed < STATIC_ROW_FRAC

    top = 0
    while top < height and static[top]:
        top += 1
    bottom = 0
    while bottom < height and static[height - 1 - bottom]:
        bottom += 1

    # Large flat backgrounds also hold still; never claim most of the screen.
    limit = int(height * MAX_STICKY_FRAC)
    return min(top, limit), min(bottom, limit)


def pick_band(content: np.ndarray, band_h: int) -> tuple[int, float]:
    """Choose a textured band near the top of a slice's content.

    A band of flat background matches at any offset, which produces a confident
    but meaningless alignment, so prefer the first band with real detail.
    """
    best = (0, -1.0)
    limit = min(content.shape[0] - band_h, 600)
    for start in range(0, max(limit, 1), 25):
        std = float(content[start:start + band_h].std())
        if std >= MIN_BAND_STD:
            return start, std
        if std > best[1]:
            best = (start, std)
    return best


def stitch(paths: list[Path], sticky_top: int | None, sticky_bottom: int | None,
           band_h: int, max_error: float, search: int,
           axis: str = "vertical") -> tuple[np.ndarray, dict]:
    horizontal = axis == "horizontal"
    rgb, gray = load(paths, horizontal)
    auto_top, auto_bottom = detect_sticky(gray)
    top = auto_top if sticky_top is None else sticky_top
    bottom = auto_bottom if sticky_bottom is None else sticky_bottom
    height = gray[0].shape[0]
    keep = height - bottom
    content_h = keep - top
    if content_h < band_h * 2:
        sys.exit(
            f"cropping {top}px off the top and {bottom}px off the bottom leaves only "
            f"{content_h}px of content per {height}px slice. --sticky-top and "
            f"--sticky-bottom are pixel COUNTS of fixed chrome, not row indices."
        )

    acc = rgb[0][:keep]          # first slice keeps its real header
    acc_g = gray[0][:keep]
    seams = []

    for idx, (path, r, g) in enumerate(zip(paths[1:], rgb[1:], gray[1:])):
        # Distinguishes "the screen never moved" from "it moved but would not
        # align" when a seam fails.
        vs_previous = float(np.abs(gray[idx] - g).mean())
        content_rgb, content_g = r[top:keep], g[top:keep]
        band_start, band_std = pick_band(content_g, band_h)
        band = content_g[band_start:band_start + band_h]

        # Every slice must extend the image. Without this floor the matcher can
        # align a slice's pinned header against the previous slice's header and
        # splice near row zero, collapsing the whole stitch to a single screen.
        min_advance = int(content_g.shape[0] * MIN_ADVANCE_FRAC)
        lo = max(min_advance + band_start, acc_g.shape[0] - search)

        def search_from(min_rows: int) -> tuple[float, int]:
            best = (float("inf"), -1)
            hi = acc_g.shape[0] - min_rows
            for cand in range(lo, max(hi, lo + 1)):
                avail = min(band_h, acc_g.shape[0] - cand)
                if avail < min_rows:
                    continue
                e = float(np.abs(acc_g[cand:cand + avail] - band[:avail]).mean())
                if e < best[0]:
                    best = (e, cand)
            return best

        # Prefer a full-band match. Fewer rows compared means a noisier score, and
        # a lucky 60-row match will beat the true 200-row one if both compete on
        # raw error. Partial overlap is a fallback for when a scroll advanced
        # nearly a full screen, not an equal candidate.
        err, p = search_from(band_h)
        partial = False
        if err > max_error:
            alt_err, alt_p = search_from(MIN_MATCH_ROWS)
            if alt_err < err:
                err, p, partial = alt_err, alt_p, True
        splice_at = p - band_start

        spliced = err <= max_error and 0 <= splice_at <= acc_g.shape[0]
        if spliced:
            acc = np.vstack([acc[:splice_at], content_rgb])
            acc_g = np.vstack([acc_g[:splice_at], content_g])
        else:
            acc = np.vstack([acc, content_rgb])
            acc_g = np.vstack([acc_g, content_g])

        seams.append({
            "slice": path.name,
            "spliced": spliced,
            "error": round(err, 2),
            "splice_row": int(splice_at),
            "band_start": int(band_start),
            "band_std": round(band_std, 1),
            "partial_overlap": partial,
            "vs_previous_diff": round(vs_previous, 2),
        })

    if horizontal:
        acc = acc.transpose(1, 0, 2)

    verdict = {
        "axis": axis,
        "slices": len(paths),
        "sticky_top": int(top),
        "sticky_bottom": int(bottom),
        "sticky_detected": {"top": int(auto_top), "bottom": int(auto_bottom)},
        "seams": seams,
        "all_spliced": all(s["spliced"] for s in seams),
    }
    return acc, verdict


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--slices", nargs="+", required=True, type=Path,
                    help="slice PNGs in scroll order, top of screen first")
    ap.add_argument("--out", required=True, type=Path, help="output PNG path")
    ap.add_argument("--sticky-top", type=int, default=None,
                    help="pixels of fixed chrome at the top (default: auto-detect)")
    ap.add_argument("--sticky-bottom", type=int, default=None,
                    help="pixels of fixed chrome at the bottom (default: auto-detect)")
    ap.add_argument("--band", type=int, default=DEFAULT_BAND)
    ap.add_argument("--max-error", type=float, default=DEFAULT_MAX_ERROR)
    ap.add_argument("--search", type=int, default=DEFAULT_SEARCH)
    ap.add_argument("--axis", choices=["vertical", "horizontal"], default="vertical",
                    help="scroll axis the slices were captured along (default: vertical). "
                         "For horizontal, --sticky-top/--sticky-bottom mean left/right.")
    args = ap.parse_args()

    missing = [str(p) for p in args.slices if not p.is_file()]
    if missing:
        sys.exit("missing slices: " + ", ".join(missing))

    image, verdict = stitch(args.slices, args.sticky_top, args.sticky_bottom,
                            args.band, args.max_error, args.search, args.axis)
    args.out.parent.mkdir(parents=True, exist_ok=True)

    # Write then rename, so a reader never sees a half-written PNG and a failed
    # run never destroys the previous capture of the same screen.
    replaced = args.out.exists()
    staged = args.out.with_name(args.out.name + ".staging")
    Image.fromarray(image).save(staged, format="PNG")  # extension is .staging, so be explicit
    staged.replace(args.out)

    verdict["out"] = str(args.out)
    verdict["replaced_existing"] = replaced
    verdict["width"] = int(image.shape[1])
    verdict["height"] = int(image.shape[0])
    print(json.dumps(verdict, indent=2))
    return 0 if verdict["all_spliced"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
