// ABOUTME: Tests the analyze-tweets scripts on a tiny fixture: cleaning rules and date range,
// ABOUTME: topic counts and timeline, chunking, and the summary id/quote verification.
import { test } from "node:test";
import assert from "node:assert/strict";
import { clean, facts, readLog } from "./clean.mjs";
import { writeFileSync, mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { chunk } from "./chunk.mjs";
import { verifySummary, tally, byFeature, byTopic, topTexts, norm, readLabels, inWindow, timeline } from "./aggregate.mjs";
import { mergeLabels, growVocab, applyAliases } from "./merge-labels.mjs";

const id = (n) => "200000000000000000" + n; // 19-digit snowflake-shaped ids
const T = (n, author, text, day, likes = 0) => ({
  id: id(n), author, text, likes, replies: 0, lang: "en",
  created_at: `Mon Sep ${day} 10:00:00 +0000 2026`,
});
const fixture = [
  T(1, "alice", "acme app is the cleanest UI in crypto", 2, 50),
  T(2, "bot1", "🚨 CTO SIGNAL Token: X Route: Acme → acme_amm", 2),
  T(3, "spam", "@a @b @c @d @e @f @g gm", 3),
  T(4, "bob", "Acme app is the cleanest UI in crypto https://t.co/x", 3), // dupe of 1
  T(5, "carol", "no airdrop, they promised it a year ago", 4, 9),
  T(6, "dave", "gm", 4),
  T(7, "erin", "fees are 0% now, nice", 5, 3),
];

test("clean drops bots, mass tags, dupes, stubs and honors --since/--until", () => {
  const { clean: c, dropped } = clean(fixture);
  assert.deepEqual(c.map((t) => t.id), [id(1), id(5), id(7)]);
  assert.deepEqual(dropped, { outOfRange: 0, bot: 1, massTag: 1, dupe: 1, short: 1 });
  const ranged = clean(fixture, undefined, "2026-09-04", "2026-09-05").clean;
  assert.deepEqual(ranged.map((t) => t.id), [id(5)]);
  const f = facts(fixture, c);
  assert.equal(f.raw, 7); assert.equal(f.clean, 3);
  assert.equal(f.from, "2026-09-02"); assert.equal(f.to, "2026-09-05");
});

test("timeline lists the top posts per day", () => {
  const c = clean(fixture).clean;
  const tl = timeline(c, 1);
  assert.deepEqual(tl.map((d) => [d.day, d.n, d.top[0].id]), [
    ["2026-09-02", 1, id(1)], ["2026-09-04", 1, id(5)], ["2026-09-05", 1, id(7)],
  ]);
});

test("chunk covers every post exactly once and keeps only labeler fields", () => {
  const chunks = chunk(fixture, 3);
  assert.equal(chunks.length, 3);
  assert.deepEqual(chunks.flat().map((t) => t.id), fixture.map((t) => t.id));
  assert.deepEqual(Object.keys(chunks[0][0]), ["id", "author", "likes", "replies", "date", "lang", "text"]);
});

test("aggregate verifies ids and quotes and tallies labels", () => {
  const byId = new Map(fixture.map((t) => [t.id, t]));
  const md = `good: ${id(1)} "cleanest UI in crypto". bad: ${id(9)} "made up quote for sure"`;
  const v = verifySummary(md, fixture, byId);
  assert.deepEqual(v.unknownIds, [id(9)]);
  assert.deepEqual(v.unverified, ["made up quote for sure"]);
  assert.equal(norm("They’re “here”… done."), "they're \"here\"… done");
  const labels = [
    { id: id(1), about: true, sentiment: "like", feature: "ui", point: "cleanest UI", request: null },
    { id: id(5), about: true, sentiment: "dislike", feature: "airdrop", point: "airdrop promised, not delivered", request: "ship the airdrop" },
    { id: id(7), about: true, sentiment: "like", feature: "fees", point: "0% fees", request: null },
  ];
  assert.deepEqual(tally(labels), { like: 2, dislike: 1 });
  assert.deepEqual(byFeature(labels.filter((l) => l.sentiment === "like"), byId), [
    { feature: "ui", count: 1, authors: 1 }, { feature: "fees", count: 1, authors: 1 },
  ]);
  assert.deepEqual(topTexts(labels, "request"), [{ text: "ship the airdrop", count: 1 }]);
  const topics = labels.map((l, i) => ({ ...l, topic: i === 1 ? "funding-revenue-growth" : "product-features" }));
  assert.deepEqual(byTopic(topics, byId), [
    { topic: "product-features", count: 2, like: 2, dislike: 0, peak: "2026-09" },
    { topic: "funding-revenue-growth", count: 1, like: 0, dislike: 1, peak: "2026-09" },
  ]);
});

test("chunk skips posts that already have a label and keeps only labeler fields", () => {
  const labeled = new Set([id(1), id(7)]);
  const chunks = chunk(fixture, 3, labeled);
  assert.deepEqual(chunks.flat().map((t) => t.id), fixture.filter((t) => !labeled.has(t.id)).map((t) => t.id));
  assert.deepEqual(Object.keys(chunks[0][0]), ["id", "author", "likes", "replies", "date", "lang", "text"]);
  assert.equal(chunk(fixture, 3).flat().length, fixture.length);
  assert.deepEqual(chunk(fixture, 3, new Set(), "2026-09-03", "2026-09-05").flat().map((t) => t.id), [id(3), id(4), id(5), id(6)]);
});

test("mergeLabels appends chunk labels into labels.jsonl, last write per id wins", () => {
  const dir = mkdtempSync(join(tmpdir(), "labels-"));
  const store = join(dir, "labels.jsonl");
  writeFileSync(store, JSON.stringify({ id: id(1), about: true, sentiment: "like", feature: "ui", point: "old", request: null }) + "\n");
  writeFileSync(join(dir, "labels0.json"), JSON.stringify([
    { id: id(1), about: true, sentiment: "like", feature: "ui", point: "cleanest UI", request: null },
    { id: id(5), about: true, sentiment: "dislike", feature: "airdrop", point: "no airdrop", request: "ship the airdrop" },
  ]));
  const { batch, ...r } = mergeLabels(store, dir, { feature: { airdrop: "airdrop-promise" } });
  assert.deepEqual(r, { files: 1, added: 1, replaced: 1, renamed: 1, total: 2 });
  const rows = readLabels(store);
  assert.deepEqual(rows.map((l) => [l.id, l.point, l.feature]), [[id(1), "cleanest UI", "ui"], [id(5), "no airdrop", "airdrop-promise"]]);
  const vocab = { features: ["ui"] };
  const g = growVocab(rows, vocab, 0.5);
  assert.deepEqual(g, { added: { topics: [], features: [["airdrop-promise", 1]], interests: [] }, below: { topics: [], features: [], interests: [] }, threshold: 1 });
  assert.deepEqual(vocab.features, ["ui", "airdrop-promise"]);
  assert.equal(applyAliases(rows, { feature: { ui: "user-interface" } }), 1);
  assert.equal(rows[0].feature, "user-interface");
});

test("inWindow filters labels by the post's day", () => {
  const byId = new Map(fixture.map((t) => [t.id, t]));
  const rows = [{ id: id(1) }, { id: id(5) }, { id: id(7) }];
  assert.deepEqual(rows.filter((l) => inWindow(l, byId, "2026-09-04", "2026-09-05")).map((l) => l.id), [id(5)]);
  assert.equal(rows.filter((l) => inWindow(l, byId)).length, 3);
});

test("readLog parses tweets.jsonl and keeps the last line of a duplicated id", () => {
  const path = join(mkdtempSync(join(tmpdir(), "tweets-")), "tweets.jsonl");
  writeFileSync(path, [
    JSON.stringify({ id: "1", text: "a" }),
    JSON.stringify({ id: "2", text: "b" }),
    "",
    JSON.stringify({ id: "1", text: "a2" }),
  ].join("\n") + "\n");
  assert.deepEqual(readLog(path), [{ id: "1", text: "a2" }, { id: "2", text: "b" }]);
});
