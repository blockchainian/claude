#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["matplotlib"]
# ///
# ABOUTME: Render the tweet-analysis charts from a spec JSON — Chinese labels, transparent, dark+light legible.
# ABOUTME: Horizontal bars for ranked sections plus a daily-volume chart with event labels; the agent supplies the counts.
"""Render charts for an X mentions analysis.

Reads a spec JSON (path arg, or stdin) and writes one PNG per chart into
out_dir. The agent supplies the final theme counts — this only draws them, in
the fixed house style: single-hue horizontal bars sorted descending, title
only (no subtitle/legend), transparent background, and a dual-mode neutral
gray for text/spines so the same PNG reads on both light and dark surfaces.

Spec shape:
{
  "out_dir": "abs/or/rel/path",
  "charts": [
    {"type": "bar", "file": "x-hot-topics.png", "title": "热点话题（提及条数）",
     "labels": ["...", ...], "values": [720, ...], "color": "#2a78d6"},
    {"type": "daily", "file": "x-daily-volume.png", "title": "每日提及量与当天事件",
     "days": ["09-02", ...], "values": [787, ...], "events": {"09-10": "App Store 下架"}},
    {"type": "bar", "file": "x-likes.png", "title": "最喜欢什么", ..., "color": "#1baf7a"}
  ]
}
House colors: topics/volume #2a78d6, likes #1baf7a, dislikes #eb6834.
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


def daily(path: str, title: str, days: list[str], values: list[int],
          events: dict[str, str], color: str) -> None:
    """Vertical bars per day; event labels stand upright above their day."""
    fig, ax = plt.subplots(figsize=(10, 5), dpi=150)
    ax.bar(days, values, color=color, width=0.7, zorder=3)
    for d, label in events.items():
        if d in days:
            ax.annotate(label.replace("$", r"\$"), (d, values[days.index(d)]),
                        xytext=(0, 6), textcoords="offset points", ha="center",
                        fontsize=8, color=INK, rotation=90)
    vmax = max(values) if values else 1
    ax.set_ylim(0, vmax * 1.9)
    ax.tick_params(colors=INK, labelsize=8)
    for s in ("top", "right"):
        ax.spines[s].set_visible(False)
    for s in ("left", "bottom"):
        ax.spines[s].set_color(SPINE)
    ax.set_title(title, fontsize=15, color=INK, fontweight="bold", loc="left", pad=14)
    plt.setp(ax.get_xticklabels(), rotation=45)
    fig.tight_layout()
    fig.savefig(path, transparent=True); plt.close(fig)


def main() -> None:
    spec = json.load(open(sys.argv[1], encoding="utf-8")) if len(sys.argv) > 1 else json.load(sys.stdin)
    out_dir = spec.get("out_dir", ".")
    os.makedirs(out_dir, exist_ok=True)
    setup_font()
    for c in spec["charts"]:
        path = os.path.join(out_dir, c["file"])
        if c["type"] == "daily":
            daily(path, c["title"], c["days"], c["values"], c.get("events", {}),
                  c.get("color", "#2a78d6"))
        else:
            hbar(path, c["title"], c["labels"], c["values"], c.get("color", "#2a78d6"))
        print("wrote", path)


if __name__ == "__main__":
    main()
