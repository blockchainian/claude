// ABOUTME: Tests the pure parts of profile-user.mjs: cashtag / mention / hashtag / domain extraction
// ABOUTME: and the whole-account profileStats roll-up (volume, reply ratio, cadence, engagement, labels).

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  cashtags,
  domains,
  hashtags,
  mentions,
  profileStats,
} from "../scripts/profile-user.mjs";

test("cashtags are upper-cased and bounded", () => {
  assert.deepEqual(cashtags("aping $bonk and $WIF, not $ or $toolongtickername"), ["BONK", "WIF"]);
  assert.deepEqual(cashtags(""), []);
});

test("mentions drop the author itself", () => {
  assert.deepEqual(mentions("@alice gm @bob ty @ALICE", "alice"), ["bob"]);
});

test("hashtags are lowercased", () => {
  assert.deepEqual(hashtags("#Solana #GM"), ["solana", "gm"]);
});

test("domains come from url entities or raw links, dropping t.co", () => {
  assert.deepEqual(
    domains({ urls: [{ expanded_url: "https://demo.fun/x" }, { expanded_url: "https://t.co/abc" }] }),
    ["demo.fun"],
  );
  assert.deepEqual(domains({ text: "see https://www.dexscreener.com/sol here" }), ["dexscreener.com"]);
});

const post = (id, iso, over = {}) => ({
  id, author: "kol", text: over.text ?? "hi", created_at: new Date(iso).toISOString(),
  likes: over.likes ?? 0, retweets: 0, replies: 0, quotes: 0, lang: over.lang ?? "en",
  url: `https://x.com/kol/status/${id}`,
});

test("profileStats rolls up volume, reply ratio, cadence and engagement", () => {
  const tweets = [
    post("1", "2026-01-01T10:00:00Z", { text: "gm $SOL", likes: 100 }),
    post("2", "2026-01-01T11:00:00Z", { text: "$SOL again @friend", likes: 50 }),
  ];
  const replies = [post("3", "2026-01-02T10:00:00Z", { text: "@friend ok", likes: 0 })];
  const s = profileStats("kol", tweets, replies);
  assert.equal(s.posts, 3);
  assert.equal(s.tweets, 2);
  assert.equal(s.replies, 1);
  assert.equal(s.reply_ratio, 0.333);
  assert.equal(s.active_days, 2);
  assert.equal(s.span_days, 2);
  assert.equal(s.engagement.max, 100);
  assert.equal(s.engagement.median, 50);
  assert.deepEqual(s.top_cashtags[0], { value: "SOL", count: 2 });
  assert.deepEqual(s.top_mentions[0], { value: "friend", count: 2 });
  assert.equal(s.hours_utc[10], 2);
  assert.equal(s.hours_utc[11], 1);
});

test("profileStats orders the span by time, not by the archive's weekday-first date strings", () => {
  // fetch-x-user-posts writes X's own format ("Wed Sep 24 12:15:26 +0000 2025"); a plain sort puts Wed before Thu.
  const tweets = [
    { ...post("1", "2025-09-24T12:00:00Z"), created_at: "Wed Sep 24 12:15:26 +0000 2025" },
    { ...post("2", "2026-09-17T07:00:00Z"), created_at: "Thu Sep 17 07:28:35 +0000 2026" },
  ];
  const s = profileStats("kol", tweets, []);
  assert.equal(s.first_post, "Wed Sep 24 12:15:26 +0000 2025");
  assert.equal(s.last_post, "Thu Sep 17 07:28:35 +0000 2026");
  assert.equal(s.span_days, 359);
});

test("profileStats folds in labels when present", () => {
  const tweets = [post("1", "2026-01-01T10:00:00Z"), post("2", "2026-01-01T11:00:00Z")];
  const labels = new Map([
    ["1", { id: "1", about: true, kind: "pnl", topic: "trade-results", asset: "SOL", stance: "bullish", interest: "referral" }],
    ["2", { id: "2", about: true, kind: "pnl", topic: "trade-results", asset: "SOL", stance: "neutral", interest: null }],
  ]);
  const s = profileStats("kol", tweets, [], labels);
  assert.equal(s.labels.labeled, 2);
  assert.deepEqual(s.labels.topic[0], { value: "trade-results", count: 2 });
  assert.deepEqual(s.labels.interest[0], { value: "referral", count: 1 });
});

test("profileStats tallies the KOL labels: kind, asset with stance, interest", () => {
  const tweets = [post("1", "2026-01-01T10:00:00Z"), post("2", "2026-01-01T11:00:00Z"), post("3", "2026-01-02T11:00:00Z")];
  const labels = new Map([
    ["1", { id: "1", about: true, kind: "call", topic: "token-call", asset: "BNB", stance: "bullish", interest: "exchange-affiliate" }],
    ["2", { id: "2", about: true, kind: "analysis", topic: "market-macro", asset: "BNB", stance: "bullish", interest: null }],
    ["3", { id: "3", about: false, kind: "noise", topic: "none", asset: "none", stance: "none", interest: null }],
  ]);
  const s = profileStats("kol", tweets, [], labels);
  assert.equal(s.labels.labeled, 3);
  assert.equal(s.labels.about, 2);
  assert.deepEqual(s.labels.kind, [{ value: "call", count: 1 }, { value: "analysis", count: 1 }, { value: "noise", count: 1 }]);
  assert.deepEqual(s.labels.asset, [{ value: "BNB", count: 2, bullish: 2, bearish: 0, neutral: 0 }]);
  assert.deepEqual(s.labels.interest, [{ value: "exchange-affiliate", count: 1 }]);
  assert.deepEqual(s.labels.stance, [{ value: "bullish", count: 2 }, { value: "none", count: 1 }]);
});

test("kol-spec declares matching fields and schema, its own vocab lists and noise rule", async () => {
  const spec = await import("../scripts/kol-spec.mjs");
  assert.deepEqual(spec.FIELDS, ["id", "about", "kind", "topic", "asset", "stance", "point", "interest"]);
  assert.deepEqual(spec.SCHEMA.properties.labels.items.required, ["n", ...spec.FIELDS.slice(1)]);
  assert.deepEqual(spec.SCHEMA.properties.labels.items.properties.stance.enum, ["bullish", "bearish", "neutral", "none"]);
  assert.deepEqual(spec.VOCAB, { topic: "topics", interest: "interests" });
  assert.ok(spec.isNoise({ kind: "noise" }) && !spec.isNoise({ kind: "call" }));
  assert.match(spec.RULES, /no app/i);
  const f = spec.fields({ topics: ["market-macro"], interests: ["own-token"] });
  assert.ok(f.includes("Examples: market-macro") && f.includes("own-token"));
});
