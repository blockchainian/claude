// ABOUTME: Tests the review-stats grounding script on a small synthetic reviews JSON.
// ABOUTME: Covers the stats JSON shape and values, Python-compatible rounding, the neg/mid/pos dumps and the CLI.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, describe, test } from "node:test";
import { analysisDir, computeStats, formatDump, formatStats, pyRound } from "../scripts/stats.mjs";

const script = fileURLToPath(new URL("../scripts/stats.mjs", import.meta.url));

const reviews = () => [
  { rating: 5, title: "Great\napp", body: "Love it 很好", date: "2024-03-01T10:00:00Z", country: "us", developerResponseBody: "thanks" },
  { rating: "4", title: "Good", body: "Nice\nwork", date: "2023-12-25", country: "gb" },
  { rating: 3, title: "Meh", body: "ok", date: "2024-01-15", country: "us" },
  { rating: 1, title: null, body: "Scam!", date: "2024-02-01", country: "de" },
  { rating: 2, title: "Bad", body: "", date: "", country: "us", developerResponseBody: "" },
  { rating: 1, title: "Worst", body: "never again", country: "fr" },
  { rating: 4.7, title: "Float", body: "x", date: "2024-05-05", country: "gb" },
  { rating: 5, title: "No country", body: "y", date: "2024-06-01" },
];

describe("computeStats", () => {
  test("counts, averages and geography match the input", () => {
    const { out, neg, mid, pos } = computeStats(reviews());
    assert.deepEqual(Object.keys(out), [
      "total", "ratings", "avg", "us", "nonUs", "usPct", "countries",
      "topCountries", "dateRange", "developerResponses", "n_dislike_1_3", "n_like_4_5",
    ]);
    assert.equal(out.total, 8);
    assert.deepEqual(out.ratings, { 1: 2, 2: 1, 3: 1, 4: 2, 5: 2 });
    assert.equal(out.avg, 3.12); // 3.125 rounds half to even, as Python does
    assert.equal(out.us, 3);
    assert.equal(out.nonUs, 5);
    assert.equal(out.usPct, 38);
    assert.equal(out.countries, 5); // a missing country counts as its own bucket
    assert.deepEqual(out.topCountries, [["us", 3], ["gb", 2], ["de", 1], ["fr", 1], [null, 1]]);
    assert.deepEqual(out.dateRange, ["2023-12-25", "2024-06-01"]);
    assert.equal(out.developerResponses, 1);
    assert.equal(out.n_dislike_1_3, 4);
    assert.equal(out.n_like_4_5, 4);
    assert.deepEqual([neg.length, mid.length, pos.length], [3, 1, 4]);
  });

  test("coerces string and float ratings to integers", () => {
    const { out } = computeStats([{ rating: "4" }, { rating: 4.7 }, { rating: null }]);
    assert.deepEqual(out.ratings, { 1: 0, 2: 0, 3: 0, 4: 2, 5: 0 });
    assert.equal(out.total, 3);
    assert.equal(out.avg, 2.67);
  });

  test("dateRange is null without dates", () => {
    assert.equal(computeStats([{ rating: 5 }]).out.dateRange, null);
  });
});

describe("pyRound", () => {
  test("ties go to even like Python's round", () => {
    assert.equal(pyRound(3.125, 2), 3.12);
    assert.equal(pyRound(3.375, 2), 3.38);
    assert.equal(pyRound(2.5), 2);
    assert.equal(pyRound(3.5), 4);
    assert.equal(pyRound(37.5), 38);
  });

  test("non-ties round normally", () => {
    assert.equal(pyRound(2.675, 2), 2.67); // 2.675 is below the tie in binary, as in Python
    assert.equal(pyRound(4.126, 2), 4.13);
    assert.equal(pyRound(4, 2), 4);
  });
});

describe("analysisDir", () => {
  test("puts the analysis under the output root, named after the reviews file", () => {
    assert.equal(analysisDir("/state/reviews/fomo.json", { INTEL_OUTPUT_DIR: "/out" }), "/out/reviews/fomo");
  });
});

describe("formatting", () => {
  test("formatStats prints an integral avg as a float", () => {
    const { out } = computeStats([{ rating: 4 }, { rating: 4 }]);
    assert.match(formatStats(out), /^  "avg": 4\.0,$/m);
    assert.match(formatStats(computeStats([{ rating: 4 }, { rating: 3 }]).out), /^  "avg": 3\.5,$/m);
  });

  test("formatDump writes one review per line with newlines flattened", () => {
    const { neg, pos } = computeStats(reviews());
    assert.equal(formatDump(neg), "[1|de]  :: Scam!\n[2|us] Bad :: \n[1|fr] Worst :: never again\n");
    assert.match(formatDump(pos), /^\[5\|us\] Great app :: Love it 很好\n/);
    assert.match(formatDump(pos), /\[5\|None\] No country :: y\n$/);
  });
});

describe("cli", () => {
  let tmp;
  before(() => {
    tmp = mkdtempSync(join(tmpdir(), "appstore-stats-"));
    writeFileSync(join(tmp, "reviews.json"), JSON.stringify({ reviews: reviews() }));
    writeFileSync(join(tmp, "empty.json"), JSON.stringify({ reviews: [] }));
  });
  after(() => rmSync(tmp, { recursive: true, force: true }));

  const run = (...args) => spawnSync(process.execPath, [script, ...args], { cwd: tmp, encoding: "utf-8", env: { ...process.env, INTEL_OUTPUT_DIR: "/out" } });

  test("prints the stats JSON and writes the three dumps", () => {
    const res = run("reviews.json", "--dump-dir", "dump");
    assert.equal(res.status, 0, res.stderr);
    const out = JSON.parse(res.stdout);
    assert.equal(out.total, 8);
    assert.equal(out.avg, 3.12);
    assert.equal(out.outDir, join("/out", "reviews", "reviews"));
    assert.equal(res.stderr, "\nwrote neg(3) mid(1) pos(4) to dump\n");
    // neg.txt is sorted by rating (1★ first), stable within a rating
    assert.equal(readFileSync(join(tmp, "dump", "neg.txt"), "utf-8"), "[1|de]  :: Scam!\n[1|fr] Worst :: never again\n[2|us] Bad :: \n");
    assert.equal(readFileSync(join(tmp, "dump", "mid.txt"), "utf-8"), "[3|us] Meh :: ok\n");
    assert.equal(readFileSync(join(tmp, "dump", "pos.txt"), "utf-8").split("\n").length - 1, 4);
  });

  test("exits 1 on an empty dataset", () => {
    const res = run("empty.json");
    assert.equal(res.status, 1);
    assert.equal(res.stderr.trim(), "no reviews found in empty.json");
  });

  test("exits 2 on a missing positional", () => {
    const res = run();
    assert.equal(res.status, 2);
    assert.match(res.stderr, /the following arguments are required: json/);
  });

  test("runs when called through a symlink to its folder, as an installed plugin is", () => {
    symlinkSync(join(script, ".."), join(tmp, "linked-scripts"));
    const res = spawnSync(process.execPath, [join(tmp, "linked-scripts", "stats.mjs")], { cwd: tmp, encoding: "utf-8" });
    assert.match(res.stderr, /the following arguments are required: json/);
  });
});
