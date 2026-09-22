// ABOUTME: Tests the analyze-tweets scripts on a tiny fixture: cleaning rules and date range,
// ABOUTME: topic counts and timeline, chunking, and the summary id/quote verification.
import { test } from "node:test";
import assert from "node:assert/strict";
import { clean, facts } from "./clean.mjs";
import { countTopics, timeline } from "./topics.mjs";
import { chunk } from "./chunk.mjs";
import { verifySummary, tally, byTopic, norm } from "./aggregate.mjs";

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

test("topics count hits, authors, likes and peak day; timeline lists top posts per day", () => {
  const c = clean(fixture).clean;
  const rows = countTopics(c, { 空投: "airdrop", 手续费: "\\bfees?\\b", 无: "zzz" });
  assert.deepEqual(rows.map((r) => [r.topic, r.n, r.authors, r.likes, r.peakDay]), [
    ["空投", 1, 1, 9, "2026-09-04"], ["手续费", 1, 1, 3, "2026-09-05"], ["无", 0, 0, 0, null],
  ]);
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
    { id: id(1), about: true, sentiment: "like", topic: "ui" },
    { id: id(5), about: true, sentiment: "dislike", topic: "airdrop" },
    { id: id(7), about: true, sentiment: "like", topic: "fees" },
  ];
  assert.deepEqual(tally(labels), { like: 2, dislike: 1 });
  assert.deepEqual(byTopic(labels.filter((l) => l.sentiment === "like"), byId), [
    { topic: "ui", count: 1, authors: 1 }, { topic: "fees", count: 1, authors: 1 },
  ]);
});
