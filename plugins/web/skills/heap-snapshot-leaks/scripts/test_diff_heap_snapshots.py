#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///
# ABOUTME: Regression tests for diff_heap_snapshots.py, using synthetic V8 .heapsnapshot pairs.
# ABOUTME: Builds a known object-count growth and detached-node set and asserts the diff reports them.
"""Run: ./test_diff_heap_snapshots.py

A leak shows up as a constructor whose live instance count and retained bytes
climb between two snapshots taken around a repeated action, and as detached DOM
nodes the page still holds. These cases build exactly that and check the report.
"""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

SCRIPT = Path(__file__).with_name("diff_heap_snapshots.py")
NODE_FIELDS = ["type", "name", "id", "self_size", "edge_count", "trace_node_id", "detachedness"]
NODE_TYPES0 = ["hidden", "array", "string", "object", "code", "closure", "regexp", "number",
               "native", "synthetic", "concatenated string", "sliced string", "symbol", "bigint"]


def snap(objects: list[dict]) -> dict:
    strings = [""]

    def sidx(s: str) -> int:
        if s not in strings:
            strings.append(s)
        return strings.index(s)

    nodes = []
    for i, o in enumerate(objects):
        nodes += [NODE_TYPES0.index(o.get("type", "object")), sidx(o["name"]), i + 1,
                  int(o.get("self_size", 0)), 0, 0, 2 if o.get("detached") else 0]
    return {"snapshot": {"meta": {"node_fields": NODE_FIELDS, "node_types": [NODE_TYPES0]},
                         "node_count": len(objects), "edge_count": 0},
            "nodes": nodes, "edges": [], "strings": strings}


def run(before: dict, after: dict, td: Path, *flags: str) -> dict:
    a, b = td / "a.heapsnapshot", td / "b.heapsnapshot"
    a.write_text(json.dumps(before))
    b.write_text(json.dumps(after))
    proc = subprocess.run([str(SCRIPT), "--before", str(a), "--after", str(b), *flags],
                          capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise SystemExit(f"diff_heap_snapshots.py failed ({proc.returncode}): {proc.stderr}")
    return json.loads(proc.stdout)


def main() -> int:
    fails: list[str] = []

    def check(cond: bool, msg: str) -> None:
        if not cond:
            fails.append(msg)

    with tempfile.TemporaryDirectory() as t:
        td = Path(t)

        before = snap([{"type": "object", "name": "Widget", "self_size": 100}] * 3
                      + [{"type": "object", "name": "App", "self_size": 40}])
        after = snap([{"type": "object", "name": "Widget", "self_size": 100}] * 8
                     + [{"type": "object", "name": "App", "self_size": 40}]
                     + [{"type": "native", "name": "Detached HTMLDivElement", "self_size": 60, "detached": True},
                        {"type": "native", "name": "Detached HTMLDivElement", "self_size": 60, "detached": True}])

        r = run(before, after, td)

        widget = next((g for g in r["growth"] if g["constructor"] == "Widget"), None)
        check(widget is not None, f"Widget growth not reported: {r['growth']}")
        if widget:
            check(widget["countDelta"] == 5, f"Widget countDelta {widget['countDelta']} != 5")
            check(widget["sizeDeltaBytes"] == 500, f"Widget sizeDeltaBytes {widget['sizeDeltaBytes']} != 500")

        check(r["detached"]["after"] == 2, f"detached after {r['detached']['after']} != 2")
        check(r["detached"]["delta"] == 2, f"detached delta {r['detached']['delta']} != 2")

        # App did not change -> not a suspect.
        app = next((g for g in r["growth"] if g["constructor"] == "App"), None)
        check(app is None or app["countDelta"] == 0, f"App wrongly flagged: {app}")

        # growth is ranked by retained-byte delta, biggest first.
        deltas = [g["sizeDeltaBytes"] for g in r["growth"]]
        check(deltas == sorted(deltas, reverse=True), f"growth not ranked by size delta: {deltas}")

        # --min-size-delta filters small movers.
        r2 = run(before, after, td, "--min-size-delta", "600")
        check(all(abs(g["sizeDeltaBytes"]) >= 600 for g in r2["growth"]),
              f"min-size-delta not applied: {r2['growth']}")

        # totals reflect the added bytes: 5 Widgets (500) + 2 detached (120).
        check(r["totalSelfSizeDeltaBytes"] == 620, f"total delta {r['totalSelfSizeDeltaBytes']} != 620")

    for f in fails:
        print("FAIL:", f)
    print("PASS: constructor growth, detached nodes, ranking and totals are reported correctly"
          if not fails else f"{len(fails)} failure(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
