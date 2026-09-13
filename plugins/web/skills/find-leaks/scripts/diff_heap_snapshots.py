#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///
# ABOUTME: Diffs two V8 .heapsnapshot files into a ranked report of what grew and what is detached.
# ABOUTME: A leak is a constructor whose instance count and retained bytes climb, plus detached DOM nodes.
"""Diff two heap snapshots taken around a repeated action to find web memory leaks.

Take one snapshot at a clean baseline, do the suspect action N times (open and
close a modal, navigate away and back), force GC, take a second snapshot. A leak
is what the heap keeps that the baseline did not: a constructor whose live
instance count and retained bytes climbed roughly in step with N, and DOM nodes
still referenced after they left the document ("detached").

Reads no browser — it parses the two JSON snapshots. Prints JSON.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path


def load(path: Path) -> tuple[dict[str, list[int]], int, dict[str, int]]:
    """Return per-constructor [count, self_size], detached count, and detached-by-type."""
    if not path.is_file():
        sys.exit(f"missing snapshot: {path}")
    try:
        data = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        sys.exit(f"{path.name} is not valid JSON: {exc}")
    meta = data["snapshot"]["meta"]
    fields = meta["node_fields"]
    stride = len(fields)
    fi = {name: i for i, name in enumerate(fields)}
    node_types = meta["node_types"][fi["type"]]
    nodes = data["nodes"]
    strings = data["strings"]
    has_detached = "detachedness" in fi

    by_ctor: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    detached_types: dict[str, int] = defaultdict(int)
    for base in range(0, len(nodes), stride):
        type_name = node_types[nodes[base + fi["type"]]]
        name = strings[nodes[base + fi["name"]]]
        size = nodes[base + fi["self_size"]]
        ctor = name if name else f"({type_name})"
        by_ctor[ctor][0] += 1
        by_ctor[ctor][1] += size
        is_detached = name.startswith("Detached ") or (has_detached and nodes[base + fi["detachedness"]] == 2)
        if is_detached:
            detached_types[ctor] += 1
    detached_total = sum(detached_types.values())
    return by_ctor, detached_total, detached_types


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--before", type=Path, required=True, help="baseline .heapsnapshot")
    ap.add_argument("--after", type=Path, required=True, help=".heapsnapshot after the repeated action")
    ap.add_argument("--top", type=int, default=25, help="constructors to report, by retained-byte growth")
    ap.add_argument("--min-size-delta", type=int, default=0,
                    help="drop constructors whose retained-byte change is smaller than this")
    args = ap.parse_args()

    before, det_before, _ = load(args.before)
    after, det_after, det_after_types = load(args.after)

    growth = []
    for ctor in set(before) | set(after):
        cb, sb = before.get(ctor, [0, 0])
        ca, sa = after.get(ctor, [0, 0])
        size_delta = sa - sb
        if abs(size_delta) < args.min_size_delta:
            continue
        growth.append({"constructor": ctor, "countBefore": cb, "countAfter": ca,
                       "countDelta": ca - cb, "sizeDeltaBytes": size_delta})
    growth.sort(key=lambda g: g["sizeDeltaBytes"], reverse=True)

    total_before = sum(v[1] for v in before.values())
    total_after = sum(v[1] for v in after.values())
    nodes_before = sum(v[0] for v in before.values())
    nodes_after = sum(v[0] for v in after.values())

    top_detached = sorted(det_after_types.items(), key=lambda kv: kv[1], reverse=True)[:10]
    report = {
        "totalSelfSizeDeltaBytes": total_after - total_before,
        "nodeCountDelta": nodes_after - nodes_before,
        "detached": {"before": det_before, "after": det_after, "delta": det_after - det_before,
                     "topTypes": [{"constructor": c, "count": n} for c, n in top_detached]},
        "growth": growth[:args.top],
        "summary": {
            "totalBytesBefore": total_before, "totalBytesAfter": total_after,
            "suspects": [g["constructor"] for g in growth[:args.top]
                         if g["countDelta"] > 0 and g["sizeDeltaBytes"] > 0][:10],
        },
    }
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
