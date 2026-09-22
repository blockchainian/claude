#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["matplotlib"]
# ///
# ABOUTME: Render the review-analysis charts from a spec JSON — Chinese labels, transparent, dark+light legible.
# ABOUTME: One bar chart per ranked section plus a sentiment-colored rating chart; the agent supplies the counts.
"""Render charts for an App Store review analysis.

Reads a spec JSON (path arg, or stdin) and writes one PNG per chart into
out_dir. The agent supplies the final theme counts — this only draws them, in
the fixed house style: single-hue horizontal bars sorted descending, title
only (no subtitle/legend), transparent background, and a dual-mode neutral
gray for text/spines so the same PNG reads on both light and dark surfaces.

Spec shape:
{
  "out_dir": "abs/or/rel/path",
  "charts": [
    {"type": "rating", "file": "x-rating-distribution.png",
     "title": "评分分布：两极分化", "values": [c1,c2,c3,c4,c5]},
    {"type": "bar", "file": "x-likes.png", "title": "最喜欢什么（4–5★）",
     "labels": ["易用 / 新手友好", ...], "values": [94, ...], "color": "#1baf7a"}
  ]
}
House colors: likes #1baf7a, dislikes #eb6834, requests #2a78d6.
"""
from __future__ import annotations

import json
import os
import sys

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib import font_manager as fm

# Dual-mode neutral gray: legible on both light and dark backgrounds.
INK = "#767676"
SPINE = "#9a9a9a"

CJK_CANDIDATES = [
    "/Library/Fonts/Arial Unicode.ttf",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
]


def setup_font() -> None:
    for p in CJK_CANDIDATES:
        if os.path.exists(p):
            fm.fontManager.addfont(p)
            plt.rcParams["font.family"] = fm.FontProperties(fname=p).get_name()
            break
    plt.rcParams.update({
        "axes.unicode_minus": False,
        "figure.facecolor": "none",
        "axes.facecolor": "none",
        "savefig.transparent": True,
    })


def hbar(path: str, title: str, labels: list[str], values: list[float], color: str) -> None:
    labels, values = labels[::-1], values[::-1]  # largest on top
    h = 0.7 + 0.44 * len(labels)
    fig, ax = plt.subplots(figsize=(8.6, h), dpi=150)
    y = range(len(labels))
    ax.barh(y, values, color=color, height=0.62, zorder=3)
    ax.set_yticks(list(y)); ax.set_yticklabels(labels, fontsize=12, color=INK)
    vmax = max(values) if values else 1
    for i, v in zip(y, values):
        ax.text(v + vmax * 0.012, i, str(v), va="center", ha="left",
                fontsize=11.5, color=INK, fontweight="bold")
    ax.set_xlim(0, vmax * 1.10)
    for s in ("top", "right", "bottom"):
        ax.spines[s].set_visible(False)
    ax.spines["left"].set_color(SPINE)
    ax.set_xticks([]); ax.tick_params(length=0)
    ax.set_title(title, fontsize=15, color=INK, fontweight="bold", loc="left", pad=14)
    fig.subplots_adjust(left=0.28, right=0.97, top=1 - 0.62 / h, bottom=0.05)
    fig.savefig(path, transparent=True); plt.close(fig)


def rating(path: str, title: str, values: list[int]) -> None:
    labels = ["1星", "2星", "3星", "4星", "5星"]
    colors = ["#e34948", "#e34948", "#9a9892", "#1baf7a", "#1baf7a"]
    fig, ax = plt.subplots(figsize=(8.6, 3.4), dpi=150)
    x = range(5)
    ax.bar(x, values, color=colors, width=0.66, zorder=3)
    ax.set_xticks(list(x)); ax.set_xticklabels(labels, fontsize=12, color=INK)
    vmax = max(values) if values else 1
    for i, v in zip(x, values):
        ax.text(i, v + vmax * 0.015, str(v), ha="center", va="bottom",
                fontsize=11.5, color=INK, fontweight="bold")
    ax.set_ylim(0, vmax * 1.13)
    for s in ("top", "right", "left"):
        ax.spines[s].set_visible(False)
    ax.spines["bottom"].set_color(SPINE)
    ax.set_yticks([]); ax.tick_params(length=0)
    ax.set_title(title, fontsize=15, color=INK, fontweight="bold", loc="left", pad=14)
    fig.subplots_adjust(left=0.04, right=0.98, top=0.86, bottom=0.11)
    fig.savefig(path, transparent=True); plt.close(fig)


def main() -> None:
    spec = json.load(open(sys.argv[1], encoding="utf-8")) if len(sys.argv) > 1 else json.load(sys.stdin)
    out_dir = spec.get("out_dir", ".")
    os.makedirs(out_dir, exist_ok=True)
    setup_font()
    for c in spec["charts"]:
        path = os.path.join(out_dir, c["file"])
        if c["type"] == "rating":
            rating(path, c["title"], c["values"])
        else:
            hbar(path, c["title"], c["labels"], c["values"], c.get("color", "#2a78d6"))
        print("wrote", path)


if __name__ == "__main__":
    main()
