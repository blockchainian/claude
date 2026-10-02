// ABOUTME: Regression tests for diff-heap-snapshots.mjs, using synthetic V8 .heapsnapshot pairs.
// ABOUTME: Builds a known object-count growth and detached-node set and asserts the diff reports them.
//
// Run: node --test tests/
//
// A leak shows up as a constructor whose live instance count and retained bytes
// climb between two snapshots taken around a repeated action, and as detached DOM
// nodes the page still holds. These cases build exactly that and check the report.
import { describe, test, before, after as teardown } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT = fileURLToPath(new URL("../scripts/diff-heap-snapshots.mjs", import.meta.url));
const NODE_FIELDS = ["type", "name", "id", "self_size", "edge_count", "trace_node_id", "detachedness"];
const NODE_TYPES0 = ["hidden", "array", "string", "object", "code", "closure", "regexp", "number",
                     "native", "synthetic", "concatenated string", "sliced string", "symbol", "bigint"];

// Build a minimal .heapsnapshot document from a list of {type, name, self_size, detached} objects.
function snap(objects) {
  const strings = [""];
  const stringIndex = (s) => {
    if (!strings.includes(s)) strings.push(s);
    return strings.indexOf(s);
  };
  const nodes = [];
  objects.forEach((o, i) => {
    nodes.push(NODE_TYPES0.indexOf(o.type ?? "object"), stringIndex(o.name), i + 1,
               Math.trunc(o.self_size ?? 0), 0, 0, o.detached ? 2 : 0);
  });
  return { snapshot: { meta: { node_fields: NODE_FIELDS, node_types: [NODE_TYPES0] },
                       node_count: objects.length, edge_count: 0 },
           nodes, edges: [], strings };
}

const repeat = (obj, n) => Array.from({ length: n }, () => ({ ...obj }));

function run(beforeSnap, afterSnap, dir, ...flags) {
  const a = join(dir, "a.heapsnapshot");
  const b = join(dir, "b.heapsnapshot");
  writeFileSync(a, JSON.stringify(beforeSnap));
  writeFileSync(b, JSON.stringify(afterSnap));
  const proc = spawnSync(process.execPath, [SCRIPT, "--before", a, "--after", b, ...flags], { encoding: "utf8" });
  if (proc.status !== 0) throw new Error(`diff-heap-snapshots.mjs failed (${proc.status}): ${proc.stderr}`);
  return JSON.parse(proc.stdout);
}

describe("diff-heap-snapshots", () => {
  let dir;
  let report;
  const beforeSnap = snap([...repeat({ type: "object", name: "Widget", self_size: 100 }, 3),
                           { type: "object", name: "App", self_size: 40 }]);
  const afterSnap = snap([...repeat({ type: "object", name: "Widget", self_size: 100 }, 8),
                          { type: "object", name: "App", self_size: 40 },
                          { type: "native", name: "Detached HTMLDivElement", self_size: 60, detached: true },
                          { type: "native", name: "Detached HTMLDivElement", self_size: 60, detached: true }]);

  before(() => {
    dir = mkdtempSync(join(tmpdir(), "heap-diff-"));
    report = run(beforeSnap, afterSnap, dir);
  });
  teardown(() => rmSync(dir, { recursive: true, force: true }));

  test("reports the constructor whose count and bytes grew", () => {
    const widget = report.growth.find((g) => g.constructor === "Widget");
    assert.ok(widget, `Widget growth not reported: ${JSON.stringify(report.growth)}`);
    assert.equal(widget.countDelta, 5);
    assert.equal(widget.sizeDeltaBytes, 500);
  });

  test("counts detached DOM nodes after the action", () => {
    assert.equal(report.detached.after, 2);
    assert.equal(report.detached.delta, 2);
  });

  test("does not flag a constructor that did not change", () => {
    const app = report.growth.find((g) => g.constructor === "App");
    assert.ok(app === undefined || app.countDelta === 0, `App wrongly flagged: ${JSON.stringify(app)}`);
  });

  test("ranks growth by retained-byte delta, biggest first", () => {
    const deltas = report.growth.map((g) => g.sizeDeltaBytes);
    assert.deepEqual(deltas, [...deltas].sort((a, b) => b - a));
  });

  test("--min-size-delta filters small movers", () => {
    const filtered = run(beforeSnap, afterSnap, dir, "--min-size-delta", "600");
    assert.ok(filtered.growth.every((g) => Math.abs(g.sizeDeltaBytes) >= 600),
              `min-size-delta not applied: ${JSON.stringify(filtered.growth)}`);
  });

  test("totals reflect the added bytes: 5 Widgets (500) + 2 detached (120)", () => {
    assert.equal(report.totalSelfSizeDeltaBytes, 620);
  });
});
