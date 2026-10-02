// ABOUTME: Tests reps.mjs (the per-section representative posts a profile is written from) and
// ABOUTME: check-quotes.mjs (every quote in a profile doc exists in the archive, verbatim, with a matching handle).

import assert from "node:assert/strict";
import { test } from "node:test";
import { sections } from "../scripts/reps.mjs";
import { checkQuotes } from "../scripts/check-quotes.mjs";

const post = (id, text, likes, day = "01") => ({ id, author: "kol", text, likes, retweets: 0, replies: 0, quotes: 0, created_at: `2026-01-${day}T10:00:00Z`, url: `https://x.com/kol/status/${id}` });
const posts = [
  post("1", "gm", 0),
  post("2", "BTC to 35000, shorting", 300, "05"),
  post("3", "BTC bull is over, top was 10/06", 200, "02"),
  post("4", "use my alpha link for 10% off", 50),
  post("5", "@friend lol", 5),
  post("6", "@friend join my paid & group", 8),
  post("7", "beta leaderboard top 100", 40),
];
const labels = new Map([
  ["1", { id: "1", about: false, kind: "noise", topic: "none", asset: "none", stance: "none", interest: null }],
  ["2", { id: "2", about: true, kind: "call", topic: "bitcoin-cycles", asset: "BTC", stance: "bearish", interest: null }],
  ["3", { id: "3", about: true, kind: "analysis", topic: "bitcoin-cycles", asset: "BTC", stance: "bearish", interest: null }],
  ["4", { id: "4", about: true, kind: "promo", topic: "trading-tools", asset: "alpha", stance: "bullish", interest: "referral" }],
  ["5", { id: "5", about: true, kind: "banter", topic: "personal-life", asset: "none", stance: "none", interest: null }],
  ["6", { id: "6", about: true, kind: "promo", topic: "personal-life", asset: "none", stance: "none", interest: "paid-group" }],
  ["7", { id: "7", about: true, kind: "pnl", topic: "trading-tools", asset: "beta", stance: "bullish", interest: null }],
]);

test("sections: topics ranked by count with top posts by engagement, market posts in date order, stakes grouped", () => {
  const s = sections(posts, labels, { per: 2, apps: ["alpha", "beta"] });
  assert.deepEqual(s.topics.map((t) => t.topic), ["bitcoin-cycles", "trading-tools", "personal-life"]);
  assert.deepEqual(s.topics[0].posts.map((p) => p.id), ["2", "3"]);
  assert.deepEqual(s.market.map((p) => p.id), ["3", "2"]);
  assert.deepEqual(Object.keys(s.stakes), ["referral", "paid-group"]);
  assert.deepEqual(s.stakes.referral.map((p) => p.id), ["4"]);
  assert.deepEqual(s.calls.map((p) => p.id), ["2"]);
  assert.deepEqual(s.assets.map((a) => a.asset), ["BTC", "alpha", "beta"]);
  assert.deepEqual(s.mentions[0], { handle: "friend", posts: [posts[5], posts[4]] });
  assert.deepEqual(s.apps.map((p) => p.id), ["4", "7"]);
});

test("checkQuotes: verbatim match ignoring whitespace and entities, wrong handle or id or text is reported", () => {
  const doc = [
    "> @kol：[BTC to 35000,  shorting](https://x.com/kol/status/2)",
    "> @kol：[join my paid &amp; group](https://x.com/kol/status/6)",
    "> @kol：[not in the post](https://x.com/kol/status/3)",
    "> @kol：[gm](https://x.com/kol/status/99)",
    "> @other：[gm](https://x.com/other/status/1)",
  ].join("\n");
  const bad = checkQuotes(doc, posts);
  assert.equal(bad.length, 3);
  assert.deepEqual(bad.map((b) => b.id), ["3", "99", "1"]);
  assert.deepEqual(bad.map((b) => b.reason), ["text", "missing", "handle"]);
});
