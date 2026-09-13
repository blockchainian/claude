#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
# ABOUTME: Diffs an actual screenshot against a desired one into a measured JSON report and a composite PNG.
# ABOUTME: Image diff says where and how much; an optional element list names what, for the implement-a-screen loop.
"""Compare two screenshots for the "build this screen from a reference" loop.

The image diff answers *where* and *how much*: a perceptual per-pixel colour
distance (CIELAB, CIE76) turned into diff regions. An optional element list —
the app's on-screen views, each a labelled pixel box the agent lifts from the
running app's accessibility tree — answers *what*: each region is attributed to
the view it falls on, so the report reads "the `Buy` button colour is off",
not "region (12,340) differs".

The target is perceptual, tolerance-bounded equivalence, not literal pixel
equality: device scale, the status bar, antialiasing and colour profiles all
make identical designs differ by a few units. `pass` is the loop's stop
condition — every attributed element within tolerance and no oversized
unattributed region.

Writes <out-dir>/diff.json and <out-dir>/diff.png, and prints the JSON.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

CELL = 16          # grid cell (px) for building diff regions
POS_WINDOW = 8     # px searched each way when locating a shifted element
POS_FACTOR = 0.5   # a shifted match must beat the in-place error by this much
FLAT_STD = 8.0     # grayscale std below which a patch is "blank" (missing/extra)
TOP_REGIONS = 12   # regions kept, ranked by area x severity


def load_rgb(path: Path) -> np.ndarray:
    if not path.is_file():
        sys.exit(f"missing image: {path}")
    return np.asarray(Image.open(path).convert("RGB"), dtype=np.uint8)


def rgb_to_lab(rgb: np.ndarray) -> np.ndarray:
    a = rgb.astype(np.float64) / 255.0
    lin = np.where(a <= 0.04045, a / 12.92, ((a + 0.055) / 1.055) ** 2.4)
    m = np.array([[0.4124564, 0.3575761, 0.1804375],
                  [0.2126729, 0.7151522, 0.0721750],
                  [0.0193339, 0.1191920, 0.9503041]])
    xyz = lin @ m.T / np.array([0.95047, 1.0, 1.08883])
    e, k = 0.008856, 903.3
    f = np.where(xyz > e, np.cbrt(xyz), (k * xyz + 16) / 116)
    return np.stack([116 * f[..., 1] - 16,
                     500 * (f[..., 0] - f[..., 1]),
                     200 * (f[..., 1] - f[..., 2])], axis=-1)


def hex_of(rgb_mean: np.ndarray) -> str:
    r, g, b = (int(round(float(c))) for c in rgb_mean)
    return f"#{r:02x}{g:02x}{b:02x}"


def clip_bbox(bbox: list[int], w: int, h: int) -> tuple[int, int, int, int] | None:
    try:
        x, y, bw, bh = (int(round(float(v))) for v in bbox)
    except (TypeError, ValueError):
        return None
    x0, y0 = max(0, x), max(0, y)
    x1, y1 = min(w, x + bw), min(h, y + bh)
    if x1 - x0 < 1 or y1 - y0 < 1:
        return None
    return x0, y0, x1, y1


def components(cellmask: np.ndarray) -> list[list[int]]:
    """4-connected components of a boolean cell grid, each as [r0, c0, r1, c1]."""
    seen = np.zeros_like(cellmask, dtype=bool)
    rows, cols = cellmask.shape
    boxes = []
    for r in range(rows):
        for c in range(cols):
            if not cellmask[r, c] or seen[r, c]:
                continue
            stack = [(r, c)]
            seen[r, c] = True
            r0 = r1 = r
            c0 = c1 = c
            while stack:
                cr, cc = stack.pop()
                r0, r1, c0, c1 = min(r0, cr), max(r1, cr), min(c0, cc), max(c1, cc)
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nr, nc = cr + dr, cc + dc
                    if 0 <= nr < rows and 0 <= nc < cols and cellmask[nr, nc] and not seen[nr, nc]:
                        seen[nr, nc] = True
                        stack.append((nr, nc))
            boxes.append([r0, c0, r1, c1])
    return boxes


def regions_from(de: np.ndarray, valid: np.ndarray, tol_de: float, total_valid: int) -> list[dict]:
    h, w = de.shape
    rows, cols = (h + CELL - 1) // CELL, (w + CELL - 1) // CELL
    cell_de = np.zeros((rows, cols))
    cell_hot = np.zeros((rows, cols), dtype=bool)
    for r in range(rows):
        for c in range(cols):
            ys, xs = slice(r * CELL, min(h, (r + 1) * CELL)), slice(c * CELL, min(w, (c + 1) * CELL))
            v = valid[ys, xs]
            if not v.any():
                continue
            m = float(de[ys, xs][v].mean())
            cell_de[r, c] = m
            cell_hot[r, c] = m > tol_de
    out = []
    for r0, c0, r1, c1 in components(cell_hot):
        x0, y0 = c0 * CELL, r0 * CELL
        x1, y1 = min(w, (c1 + 1) * CELL), min(h, (r1 + 1) * CELL)
        block_valid = valid[y0:y1, x0:x1]
        area = int(block_valid.sum())
        if not area:
            continue
        de_mean = float(de[y0:y1, x0:x1][block_valid].mean())
        out.append({"bbox": [x0, y0, x1 - x0, y1 - y0],
                    "areaPct": round(area / max(total_valid, 1) * 100, 3),
                    "deltaE": round(de_mean, 2)})
    out.sort(key=lambda rg: rg["areaPct"] * rg["deltaE"], reverse=True)
    return out[:TOP_REGIONS]


def gray(rgb: np.ndarray) -> np.ndarray:
    return rgb.astype(np.float64) @ np.array([0.299, 0.587, 0.114])


def find_offset(ag: np.ndarray, dg: np.ndarray, box: tuple[int, int, int, int]) -> tuple[list[int], float, float]:
    x0, y0, x1, y1 = box
    patch = ag[y0:y1, x0:x1]
    ph, pw = patch.shape
    h, w = dg.shape
    inplace = float(np.abs(patch - dg[y0:y1, x0:x1]).mean())
    best, best_err = (0, 0), inplace
    for dy in range(-POS_WINDOW, POS_WINDOW + 1):
        for dx in range(-POS_WINDOW, POS_WINDOW + 1):
            if dx == 0 and dy == 0:
                continue
            sy, sx = y0 + dy, x0 + dx
            if sy < 0 or sx < 0 or sy + ph > h or sx + pw > w:
                continue
            err = float(np.abs(patch - dg[sy:sy + ph, sx:sx + pw]).mean())
            if err < best_err:
                best, best_err = (dx, dy), err
    return [best[0], best[1]], best_err, inplace


def attribute(elements: list, lab_a: np.ndarray, lab_d: np.ndarray, rgb_a: np.ndarray,
              rgb_d: np.ndarray, ag: np.ndarray, dg: np.ndarray,
              tol_de: float, tol_px: int) -> list[dict]:
    h, w = ag.shape
    out = []
    for el in elements:
        if not isinstance(el, dict) or "bbox" not in el:
            continue
        box = clip_bbox(el["bbox"], w, h)
        if box is None:
            continue
        x0, y0, x1, y1 = box
        de = float(np.sqrt(((lab_a[y0:y1, x0:x1].mean((0, 1)) - lab_d[y0:y1, x0:x1].mean((0, 1))) ** 2).sum()))
        color_a = hex_of(rgb_a[y0:y1, x0:x1].reshape(-1, 3).mean(0))
        color_d = hex_of(rgb_d[y0:y1, x0:x1].reshape(-1, 3).mean(0))
        offset, best_err, inplace = find_offset(ag, dg, box)
        act_std = float(ag[y0:y1, x0:x1].std())
        des_std = float(dg[y0:y1, x0:x1].std())

        if act_std < FLAT_STD <= des_std:
            kind, offset = "missing", [0, 0]
        elif des_std < FLAT_STD <= act_std:
            kind, offset = "extra", [0, 0]
        elif best_err < inplace * POS_FACTOR and max(abs(offset[0]), abs(offset[1])) > tol_px:
            kind = "position"
        elif de >= tol_de:
            kind, offset = "color", [0, 0]
        else:
            kind, offset = "ok", [0, 0]

        out.append({"label": el.get("label", ""), "type": el.get("type", ""),
                    "bbox": [x0, y0, x1 - x0, y1 - y0], "deltaE": round(de, 2),
                    "colorActual": color_a, "colorDesired": color_d,
                    "offsetPx": offset, "kind": kind})
    return out


def compose(rgb_a: np.ndarray, rgb_d: np.ndarray, de: np.ndarray,
            regions: list[dict], elements: list[dict], out_path: Path) -> None:
    h, w = de.shape
    heat = (gray(rgb_a) * 0.5).astype(np.uint8)
    heat = np.stack([heat, heat, heat], axis=-1)
    strength = np.clip(de / max(float(de.max()), 1.0), 0, 1)
    heat[..., 0] = np.clip(heat[..., 0] + (strength * 205).astype(np.uint8), 0, 255)
    strip = Image.new("RGB", (w * 3, h), (0, 0, 0))
    strip.paste(Image.fromarray(rgb_a), (0, 0))
    strip.paste(Image.fromarray(rgb_d), (w, 0))
    strip.paste(Image.fromarray(heat), (w * 2, 0))
    draw = ImageDraw.Draw(strip)
    for rg in regions:
        x, y, bw, bh = rg["bbox"]
        draw.rectangle([w * 2 + x, y, w * 2 + x + bw, y + bh], outline=(255, 220, 0), width=2)
    for el in elements:
        if el["kind"] == "ok":
            continue
        x, y, bw, bh = el["bbox"]
        draw.rectangle([w * 2 + x, y, w * 2 + x + bw, y + bh], outline=(0, 200, 255), width=2)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    strip.save(out_path)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--actual", type=Path, required=True, help="screenshot of the app being built")
    ap.add_argument("--desired", type=Path, required=True, help="the target/reference screenshot")
    ap.add_argument("--out-dir", type=Path, required=True)
    ap.add_argument("--hierarchy", type=Path, default=None,
                    help="JSON list of {label,type,bbox:[x,y,w,h]} in actual-image pixels")
    ap.add_argument("--tol-de", type=float, default=3.0, help="per-element ΔE that counts as a colour miss")
    ap.add_argument("--tol-px", type=int, default=2, help="element offset (px) that counts as a position miss")
    ap.add_argument("--region-area-pct", type=float, default=0.5,
                    help="an unattributed region larger than this share of the frame fails the check")
    ap.add_argument("--mask-top", type=int, default=0, help="px of fixed chrome to ignore at the top (status bar)")
    ap.add_argument("--mask-bottom", type=int, default=0, help="px to ignore at the bottom (home indicator)")
    args = ap.parse_args()

    rgb_a = load_rgb(args.actual)
    rgb_d = load_rgb(args.desired)
    h, w = rgb_a.shape[:2]

    resized = rgb_d.shape[:2] != (h, w)
    ratio = None
    if resized:
        ratio = [round(w / rgb_d.shape[1], 4), round(h / rgb_d.shape[0], 4)]
        rgb_d = np.asarray(Image.fromarray(rgb_d).resize((w, h), Image.Resampling.BICUBIC), dtype=np.uint8)

    top = max(0, min(args.mask_top, h))
    bottom = max(0, min(args.mask_bottom, h - top))
    valid = np.zeros((h, w), dtype=bool)
    valid[top:h - bottom, :] = True

    lab_a, lab_d = rgb_to_lab(rgb_a), rgb_to_lab(rgb_d)
    de = np.sqrt(((lab_a - lab_d) ** 2).sum(axis=-1))
    de[~valid] = 0.0

    total_valid = int(valid.sum())
    vals = de[valid]
    regions = regions_from(de, valid, args.tol_de, total_valid)

    elements = []
    if args.hierarchy is not None:
        try:
            raw = json.loads(Path(args.hierarchy).read_text())
        except (OSError, json.JSONDecodeError) as exc:
            sys.exit(f"bad --hierarchy file: {exc}")
        if not isinstance(raw, list):
            sys.exit("--hierarchy must be a JSON list of elements")
        elements = attribute(raw, lab_a, lab_d, rgb_a, rgb_d, gray(rgb_a), gray(rgb_d),
                             args.tol_de, args.tol_px)

    failing = [e for e in elements if e["kind"] != "ok"]
    big_regions = [r for r in regions if r["areaPct"] > args.region_area_pct]
    passed = not failing and not big_regions

    compose(rgb_a, rgb_d, de, regions, elements, args.out_dir / "diff.png")

    report = {
        "pass": passed,
        "tolerances": {"tolDe": args.tol_de, "tolPx": args.tol_px, "regionAreaPct": args.region_area_pct},
        "normalize": {"resized": resized, "ratio": ratio, "maskTop": top, "maskBottom": bottom},
        "summary": {
            "meanDeltaE": round(float(vals.mean()), 2) if total_valid else 0.0,
            "maxDeltaE": round(float(vals.max()), 2) if total_valid else 0.0,
            "diffAreaPct": round(float((vals > args.tol_de).mean()) * 100, 3) if total_valid else 0.0,
            "nRegions": len(regions),
            "nElements": len(elements),
            "nElementsFailing": len(failing),
        },
        "elements": elements,
        "regions": regions,
    }
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / "diff.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
