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
NAV = 80                                          # navigation bar below the status bar
BAND_TOP, BAND_BOTTOM, BAND_STEP = 300, 500, 120  # a carousel inside a still screen
BAND_EDGE = 20                                    # its sparse, text-like outer rows
LIVE_TOP, LIVE_BOTTOM = 560, 800                  # a taller region of live values
TITLED_VISIBLE = VISIBLE - NAV


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


def build_horizontal_slices(tmp: Path) -> tuple[list[Path], np.ndarray]:
    """The same page rotated: a wide screen scrolled sideways.

    Fixed chrome sits on the left and right edges instead of top and bottom.
    """
    paths, page = build_slices(tmp / "v")
    wide = []
    for i, src in enumerate(paths):
        arr = np.asarray(Image.open(src).convert("RGB")).transpose(1, 0, 2)
        out = tmp / f"wide-{i:02d}.png"
        Image.fromarray(arr).save(out)
        wide.append(out)
    return wide, page.transpose(1, 0, 2)


def build_large_title_slices(tmp: Path) -> tuple[list[Path], np.ndarray]:
    """Slices of a screen whose large navigation title collapses after the first.

    iOS draws an expanded title at scroll offset zero and swaps it for a compact
    title bar as soon as the page moves. The bar occupies the same rows either
    way, so it is chrome, but its pixels differ between the first slice and every
    later one.
    """
    rng = np.random.default_rng(99)
    page = rng.integers(0, 255, size=(PAGE_ROWS, WIDTH, 3), dtype=np.uint8)

    status = np.full((STICKY_TOP, WIDTH, 3), 30, dtype=np.uint8)
    status[20:40, 10:200] = 200
    expanded = rng.integers(0, 255, size=(NAV, WIDTH, 3), dtype=np.uint8)
    collapsed = rng.integers(0, 255, size=(NAV, WIDTH, 3), dtype=np.uint8)
    footer = np.full((STICKY_BOTTOM, WIDTH, 3), 60, dtype=np.uint8)

    paths = []
    for i, offset in enumerate(range(0, STEP * 4, STEP)):
        nav = expanded if i == 0 else collapsed
        body = page[offset:offset + TITLED_VISIBLE]
        p = tmp / f"titled-{i:02d}.png"
        Image.fromarray(np.vstack([status, nav, body, footer])).save(p)
        paths.append(p)
    return paths, page


def check_large_title(mod, failures: list[str]) -> None:
    with tempfile.TemporaryDirectory() as td:
        paths, page = build_large_title_slices(Path(td))
        image, verdict = mod.stitch(paths, None, None, mod.DEFAULT_BAND,
                                    mod.DEFAULT_MAX_ERROR, mod.DEFAULT_SEARCH)

        chrome = STICKY_TOP + NAV
        if verdict["sticky_top"] != chrome:
            failures.append(
                f"large title: sticky_top {verdict['sticky_top']} != {chrome} "
                "(the first slice's expanded title hid the bar below it)")
        if not verdict["all_spliced"]:
            failures.append(f"large title: not every seam spliced: {verdict['seams']}")

        expected_h = chrome + TITLED_VISIBLE + STEP * 3
        if image.shape[0] != expected_h:
            failures.append(f"large title: height {image.shape[0]} != {expected_h} "
                            "(content was dropped at a seam)")
        else:
            covered = TITLED_VISIBLE + STEP * 3
            got = image[chrome:]
            if not np.array_equal(got, page[:covered]):
                bad = int((got != page[:covered]).any(axis=(1, 2)).sum())
                failures.append(f"large title: content mismatch on {bad} rows")


def build_carousel_slices(tmp: Path) -> tuple[list[Path], np.ndarray]:
    """Full-screen slices of a screen whose only moving part scrolls sideways.

    Everything outside the band holds still, which is what makes a full-screen
    match meaningless: the static remainder aligns at any offset.
    """
    rng = np.random.default_rng(2024)
    page = rng.integers(0, 90, size=(VIEWPORT, WIDTH, 3), dtype=np.uint8)
    strip = rng.integers(0, 255,
                         size=(BAND_BOTTOM - BAND_TOP, WIDTH + BAND_STEP * 3, 3),
                         dtype=np.uint8)

    # A real carousel is a row of labels: its outer rows hold the tops and tails
    # of glyphs and are mostly background, so only a few of their pixels move.
    # Detecting the band on that share alone crops the glyphs in half.
    sparse = np.zeros((BAND_EDGE, strip.shape[1], 1), dtype=bool)
    sparse[:, ::20] = True
    for rows in (slice(0, BAND_EDGE), slice(-BAND_EDGE, None)):
        strip[rows] = np.where(sparse, strip[rows], 40)

    paths = []
    for i in range(4):
        frame = page.copy()
        frame[BAND_TOP:BAND_BOTTOM] = strip[:, BAND_STEP * i:BAND_STEP * i + WIDTH]
        # A taller region of live values: prices and sparklines that redraw
        # between frames without scrolling. It changes enough rows to form a
        # longer span than the carousel, so picking the longest span picks this.
        live = frame[LIVE_TOP:LIVE_BOTTOM]
        live[:, ::5] = rng.integers(0, 255, size=live[:, ::5].shape, dtype=np.uint8)
        q = tmp / f"carousel-{i:02d}.png"
        Image.fromarray(frame).save(q)
        paths.append(q)
    return paths, strip


def check_carousel_band(mod, failures: list[str]) -> None:
    with tempfile.TemporaryDirectory() as td:
        paths, strip = build_carousel_slices(Path(td))
        image, verdict = mod.stitch(paths, None, None, mod.DEFAULT_BAND,
                                    mod.DEFAULT_MAX_ERROR, mod.DEFAULT_SEARCH,
                                    "horizontal", True)

        if verdict["cropped_band"] != [BAND_TOP, BAND_BOTTOM]:
            failures.append(f"carousel: band {verdict['cropped_band']} != "
                            f"[{BAND_TOP}, {BAND_BOTTOM}]")
        if not verdict["all_spliced"]:
            failures.append(f"carousel: not every seam spliced: {verdict['seams']}")

        expected_w = WIDTH + BAND_STEP * 3
        if image.shape[1] != expected_w:
            failures.append(f"carousel: width {image.shape[1]} != {expected_w}")
        elif image.shape[0] != BAND_BOTTOM - BAND_TOP:
            failures.append(f"carousel: height {image.shape[0]} != "
                            f"{BAND_BOTTOM - BAND_TOP} (cropped to the wrong rows)")
        elif not np.array_equal(image, strip):
            failures.append("carousel: the reconstructed strip does not match")

        # Without the crop the same slices must NOT stitch cleanly: the still
        # screen around the band aligns anywhere. This is the failure the flag
        # exists to prevent, so it is worth pinning down.
        _, plain = mod.stitch(paths, None, None, mod.DEFAULT_BAND,
                              mod.DEFAULT_MAX_ERROR, mod.DEFAULT_SEARCH,
                              "horizontal", False)
        if plain["cropped_band"] is not None:
            failures.append("carousel: a band was cropped without the flag")


def check_horizontal(mod, failures: list[str]) -> None:
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td); (tmp / "v").mkdir()
        paths, page = build_horizontal_slices(tmp)
        image, verdict = mod.stitch(paths, None, None, mod.DEFAULT_BAND,
                                    mod.DEFAULT_MAX_ERROR, mod.DEFAULT_SEARCH,
                                    "horizontal")
        if verdict["axis"] != "horizontal":
            failures.append("axis not reported as horizontal")
        if not verdict["all_spliced"]:
            failures.append(f"horizontal: not every seam spliced: {verdict['seams']}")
        expected_w = STICKY_TOP + VISIBLE + STEP * 3
        if image.shape[1] != expected_w:
            failures.append(f"horizontal width {image.shape[1]} != {expected_w}")
        elif not np.array_equal(image[:, STICKY_TOP:], page[:, :VISIBLE + STEP * 3]):
            failures.append("horizontal: content mismatch after transpose round-trip")


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

    check_large_title(mod, failures)
    check_carousel_band(mod, failures)
    check_horizontal(mod, failures)

    for f in failures:
        print("FAIL:", f)
    print("PASS: stitch reconstructs the page exactly, both axes, a collapsing "
          "title and a carousel band" if not failures
          else f"{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
