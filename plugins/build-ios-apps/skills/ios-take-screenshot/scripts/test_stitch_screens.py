#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
"""Regression tests for stitch_screens.py, using synthetic slices.

Builds a tall page, cuts it into overlapping viewport slices with fixed chrome
on both edges, and checks that stitching reconstructs the original page exactly.
Run: ./test_stitch_screens.py
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image

WIDTH, VIEWPORT = 400, 800
STICKY_TOP, STICKY_BOTTOM = 100, 120
PAGE_ROWS, STEP = 3000, 400
VISIBLE = VIEWPORT - STICKY_TOP - STICKY_BOTTOM   # 580 rows of content per slice


def load_module():
    path = Path(__file__).with_name("stitch_screens.py")
    spec = importlib.util.spec_from_file_location("stitch_screens", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def build_slices(tmp: Path) -> tuple[list[Path], np.ndarray]:
    rng = np.random.default_rng(1234)
    page = rng.integers(0, 255, size=(PAGE_ROWS, WIDTH, 3), dtype=np.uint8)

    header = np.full((STICKY_TOP, WIDTH, 3), 30, dtype=np.uint8)
    header[20:40, 10:200] = 200                      # static label
    footer = np.full((STICKY_BOTTOM, WIDTH, 3), 60, dtype=np.uint8)

    paths = []
    for i, offset in enumerate(range(0, STEP * 4, STEP)):
        head = header.copy()
        # A live-updating value in a pinned header: a few pixels change every
        # slice, so the row's average difference is large but its changed-pixel
        # share stays small. Detection must still see this row as chrome.
        head[50:70, 300:320] = (i * 60) % 255
        slice_img = np.vstack([head, page[offset:offset + VISIBLE], footer])
        p = tmp / f"slice-{i:02d}.png"
        Image.fromarray(slice_img).save(p)
        paths.append(p)
    return paths, page


def main() -> int:
    mod = load_module()
    failures = []

    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        paths, page = build_slices(tmp)
        image, verdict = mod.stitch(paths, None, None, mod.DEFAULT_BAND,
                                    mod.DEFAULT_MAX_ERROR, mod.DEFAULT_SEARCH)

        if verdict["sticky_top"] != STICKY_TOP:
            failures.append(f"sticky_top {verdict['sticky_top']} != {STICKY_TOP} "
                            "(a pinned header with a live value was misread)")
        if verdict["sticky_bottom"] != STICKY_BOTTOM:
            failures.append(f"sticky_bottom {verdict['sticky_bottom']} != {STICKY_BOTTOM}")
        if not verdict["all_spliced"]:
            failures.append(f"not every seam spliced: {verdict['seams']}")

        rows = [s["splice_row"] for s in verdict["seams"]]
        if rows != sorted(rows) or any(r <= 0 for r in rows):
            failures.append(f"splice rows must increase and be positive: {rows}")

        expected_h = STICKY_TOP + VISIBLE + STEP * 3
        if image.shape[0] != expected_h:
            failures.append(f"height {image.shape[0]} != {expected_h} "
                            "(a collapsed stitch looks like one short screen)")

        # The strongest check: the reconstructed content must equal the original
        # page row for row, which fails if any seam duplicates or drops content.
        covered = VISIBLE + STEP * 3
        if image.shape[0] == expected_h:
            got = image[STICKY_TOP:]
            if not np.array_equal(got, page[:covered]):
                bad = int((got != page[:covered]).any(axis=(1, 2)).sum())
                failures.append(f"content mismatch on {bad} rows")

    for f in failures:
        print("FAIL:", f)
    print("PASS: stitch reconstructs the page exactly" if not failures
          else f"{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
