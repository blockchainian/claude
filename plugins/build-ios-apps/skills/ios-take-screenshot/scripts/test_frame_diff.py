#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
"""Regression tests for frame_diff.py.

The capture loop stops when a scroll stops moving the page, so the one thing
this must never do is call a ticking value a scroll.
Run: ./test_frame_diff.py
"""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image

WIDTH, HEIGHT = 400, 1200
CHROME_TOP, CHROME_BOTTOM = 100, 80
OFFSET = 500


def run(before: Path, after: Path) -> dict:
    script = Path(__file__).with_name("frame_diff.py")
    proc = subprocess.run(
        [str(script), str(before), str(after),
         "--sticky-top", str(CHROME_TOP), "--sticky-bottom", str(CHROME_BOTTOM)],
        capture_output=True, text=True, check=False,
    )
    if proc.returncode != 0:
        raise SystemExit(f"frame_diff.py failed: {proc.stderr}")
    return json.loads(proc.stdout)


def frame(page: np.ndarray, offset: int, tick: int | None = None) -> np.ndarray:
    body = page[offset:offset + HEIGHT - CHROME_TOP - CHROME_BOTTOM]
    top = np.full((CHROME_TOP, WIDTH, 3), 25, dtype=np.uint8)
    bottom = np.full((CHROME_BOTTOM, WIDTH, 3), 55, dtype=np.uint8)
    out = np.vstack([top, body, bottom])
    if tick is not None:
        # A balance that redraws in place: a big figure, and nothing moved.
        out[CHROME_TOP + 20:CHROME_TOP + 95, 30:330] = tick
    return out


def main() -> int:
    rng = np.random.default_rng(11)
    page = rng.integers(0, 255, size=(3000, WIDTH, 3), dtype=np.uint8)
    failures = []

    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)

        a, b = tmp / "a.png", tmp / "b.png"
        Image.fromarray(frame(page, 0)).save(a)
        Image.fromarray(frame(page, OFFSET)).save(b)
        r = run(a, b)
        if r["scrolled_px"] != OFFSET:
            failures.append(f"scrolled_px {r['scrolled_px']} != {OFFSET}")
        if not r["scrolled"]:
            failures.append(f"a real scroll was not reported as one: {r}")

        # The trap: the page did not move, but a value on it redrew. The raw
        # difference is well above the stop threshold; the offset is still zero.
        c, d = tmp / "c.png", tmp / "d.png"
        Image.fromarray(frame(page, 0, tick=10)).save(c)
        Image.fromarray(frame(page, 0, tick=240)).save(d)
        r = run(c, d)
        if r["scrolled_px"] != 0:
            failures.append(f"a ticking value moved the page: scrolled_px {r['scrolled_px']}")
        if r["scrolled"]:
            failures.append(f"a ticking value was reported as a scroll: {r}")
        if r["mean_abs_diff"] <= 2:
            failures.append("the tick was too small to be a meaningful test "
                            f"(diff {r['mean_abs_diff']}); it must exceed the stop threshold")

        e = tmp / "e.png"
        Image.fromarray(frame(page, 0)).save(e)
        r = run(a, e)
        if r["differs"] or r["scrolled"]:
            failures.append(f"identical frames reported as changed: {r}")

    for f in failures:
        print("FAIL:", f)
    print("PASS: a scroll reads as a scroll and a ticking value does not" if not failures
          else f"{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
