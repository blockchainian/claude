#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
# ABOUTME: Regression tests for compare_screens.py, using synthetic screenshot pairs.
# ABOUTME: Builds known colour, position, resize and mask differences and asserts the report.
"""Run: ./test_compare_screens.py

Each case builds a pair of images whose one difference is known, then checks the
report names it: a recolour as a region, a recoloured element as `color`, a
shifted element as `position` with the right offset, a resized desired as
normalized, and a masked band as ignored.
"""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image

W, H = 240, 480
SCRIPT = Path(__file__).with_name("compare_screens.py")


def bg(seed: int = 7) -> np.ndarray:
    rng = np.random.default_rng(seed)
    return rng.integers(0, 255, size=(H, W, 3), dtype=np.uint8)


def save(arr: np.ndarray, path: Path) -> None:
    Image.fromarray(arr).save(path)


def run(td: Path, actual: np.ndarray, desired: np.ndarray,
        hierarchy: list | None = None, **flags) -> tuple[dict, Path]:
    a, d = td / "actual.png", td / "desired.png"
    save(actual, a)
    save(desired, d)
    out = td / "out"
    cmd = [str(SCRIPT), "--actual", str(a), "--desired", str(d), "--out-dir", str(out)]
    if hierarchy is not None:
        h = td / "elements.json"
        h.write_text(json.dumps(hierarchy))
        cmd += ["--hierarchy", str(h)]
    for k, v in flags.items():
        cmd += [f"--{k.replace('_', '-')}", str(v)]
    proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise SystemExit(f"compare_screens.py failed ({proc.returncode}): {proc.stderr}")
    return json.loads(proc.stdout), out


def elem(r: dict, label: str) -> dict | None:
    return next((e for e in r.get("elements", []) if e["label"] == label), None)


def main() -> int:
    fails: list[str] = []

    def check(cond: bool, msg: str) -> None:
        if not cond:
            fails.append(msg)

    with tempfile.TemporaryDirectory() as t:
        td0 = Path(t)

        # 1. identical -> pass, ~zero delta, no regions.
        with tempfile.TemporaryDirectory() as t1:
            base = bg()
            r, out = run(Path(t1), base, base.copy())
            check(r["pass"] is True, f"identical images should pass: {r['summary']}")
            check(r["summary"]["maxDeltaE"] < 1.0, f"identical maxDeltaE not ~0: {r['summary']}")
            check(not r["regions"], f"identical images produced regions: {r['regions']}")
            check((out / "diff.json").is_file(), "diff.json not written")
            check((out / "diff.png").is_file(), "diff.png not written")

        # 2. one recoloured rectangle, no hierarchy -> a region there, fail.
        with tempfile.TemporaryDirectory() as t2:
            base = bg()
            des = base.copy()
            bx, by, bw, bh = 60, 140, 80, 90
            des[by:by + bh, bx:bx + bw] = [220, 30, 30]
            r, _ = run(Path(t2), base, des)
            check(r["pass"] is False, "a recoloured block should fail")
            hit = any(rg["bbox"][0] <= bx + bw and rg["bbox"][0] + rg["bbox"][2] >= bx
                      and rg["bbox"][1] <= by + bh and rg["bbox"][1] + rg["bbox"][3] >= by
                      for rg in r["regions"])
            check(hit, f"no region overlaps the recoloured block: {r['regions']}")

        # 3. an element shifted by K px, with a hierarchy -> kind position, offset ~K.
        with tempfile.TemporaryDirectory() as t3:
            base = bg()
            K = 6
            px, py, pw, ph = 40, 100, 64, 64
            patch = np.random.default_rng(99).integers(0, 255, size=(ph, pw, 3), dtype=np.uint8)
            act = base.copy()
            act[py:py + ph, px:px + pw] = patch
            des = base.copy()
            des[py:py + ph, px + K:px + K + pw] = patch
            r, _ = run(Path(t3), act, des, hierarchy=[{"label": "logo", "type": "Image", "bbox": [px, py, pw, ph]}])
            e = elem(r, "logo")
            check(e is not None, "shifted element not attributed")
            if e:
                check(e["kind"] == "position", f"shift not classified position: {e}")
                check(abs(e["offsetPx"][0] - K) <= 1 and abs(e["offsetPx"][1]) <= 1,
                      f"offset {e['offsetPx']} != ~[{K},0]")
            check(r["pass"] is False, "a shifted element should fail")

        # 4. a recoloured element, with a hierarchy -> kind color, hex right.
        with tempfile.TemporaryDirectory() as t4:
            base = bg()
            bx, by, bw, bh = 50, 200, 100, 60
            act = base.copy(); act[by:by + bh, bx:bx + bw] = [40, 160, 80]
            des = base.copy(); des[by:by + bh, bx:bx + bw] = [60, 90, 200]
            r, _ = run(Path(t4), act, des, hierarchy=[{"label": "card", "type": "View", "bbox": [bx, by, bw, bh]}])
            e = elem(r, "card")
            check(e is not None, "recoloured element not attributed")
            if e:
                check(e["kind"] == "color", f"recolour not classified color: {e}")
                check(e["colorActual"].lower() == "#28a050", f"colorActual {e['colorActual']} != #28a050")
                check(e["colorDesired"].lower() == "#3c5ac8", f"colorDesired {e['colorDesired']} != #3c5ac8")
            check(r["pass"] is False, "a recoloured element should fail")

        # 5. desired larger than actual -> normalized, still runs; same solid -> pass.
        with tempfile.TemporaryDirectory() as t5:
            act = np.full((H, W, 3), 128, dtype=np.uint8)
            des = np.full((H * 2, W * 2, 3), 128, dtype=np.uint8)
            r, _ = run(Path(t5), act, des)
            check(r["normalize"]["resized"] is True, f"resize not reported: {r['normalize']}")
            check(r["pass"] is True, f"same solid at 2x should pass after resize: {r['summary']}")

        # 6. a differing top band, masked -> ignored; unmasked -> fails.
        with tempfile.TemporaryDirectory() as t6:
            base = bg()
            M = 40
            des = base.copy()
            des[0:M, :] = np.random.default_rng(3).integers(0, 255, size=(M, W, 3), dtype=np.uint8)
            r_masked, _ = run(Path(t6), base, des, mask_top=M)
            check(r_masked["pass"] is True, f"masked band should not fail: {r_masked['summary']}")
            check(r_masked["summary"]["maxDeltaE"] < 1.0, f"masked band leaked into delta: {r_masked['summary']}")
        with tempfile.TemporaryDirectory() as t6b:
            r_unmasked, _ = run(Path(t6b), base, des)
            check(r_unmasked["pass"] is False, "an unmasked differing band should fail")

        # 7. malformed hierarchy entries are skipped, valid one still attributed.
        with tempfile.TemporaryDirectory() as t7:
            base = bg()
            bx, by, bw, bh = 50, 200, 100, 60
            act = base.copy(); act[by:by + bh, bx:bx + bw] = [40, 160, 80]
            des = base.copy(); des[by:by + bh, bx:bx + bw] = [60, 90, 200]
            hier = [
                {"label": "no-bbox", "type": "View"},
                {"label": "zero", "type": "View", "bbox": [10, 10, 0, 0]},
                {"label": "oob", "type": "View", "bbox": [10000, 10000, 50, 50]},
                {"label": "card", "type": "View", "bbox": [bx, by, bw, bh]},
            ]
            r, _ = run(Path(t7), act, des, hierarchy=hier)
            labels = {e["label"] for e in r["elements"]}
            check(labels == {"card"}, f"malformed entries not skipped cleanly: {labels}")

        _ = td0  # keep the outer temp dir alive for the run

    for f in fails:
        print("FAIL:", f)
    print("PASS: colour, position, resize and mask differences are each reported correctly"
          if not fails else f"{len(fails)} failure(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
