// ABOUTME: Tests profile-authors.mjs on a tiny fixture: per-author features from posts and labels,
// ABOUTME: role assignment, segment shares, the die-hard rule and representative-account sampling.
import { test } from "node:test";
import assert from "node:assert/strict";
import { authorFeatures, assignRoles, roleStats, dieHards, pickReps, samplePosts } from "../scripts/profile-authors.mjs";

const id = (n) => "200000000000000000" + n;
let seq = 0;
const P = (author, text, day, likes = 0, label = {}) => ({
  id: id(++seq), author, text, likes, retweets: 0, replies: 0, quotes: 0, lang: "en",
  created_at: `Mon Sep ${String(day).padStart(2, " ")} 10:00:00 +0000 2026`,
  label: { about: true, sentiment: "neutral", topic: "none", feature: "none", interest: null, ...label },
});
const posts = [
  ...[1, 2, 3].map((d) => P("promo", "trade with me on acme, 10% off", d, 1, { interest: "referral" })),
  P("promo", "acme is fine", 4, 1),
  ...[1, 2, 3, 4, 5].map((d) => P("degen", "aped $DOGE on acme, up 3x", d, 2, { topic: "trading-results" })),
  ...[1, 2, 3, 4, 5].map((d) => P("star", "acme thread", d, 500)),
  ...[1, 2, 3].map((d) => P("hater", "acme cant sell, scam", d, 0, { sentiment: "dislike" })),
  ...[1, 2, 3].map((d) => P("farmer", "rt + follow, $100 giveaway", d, 0, { topic: "challenges-giveaways", about: false, sentiment: "noise" })),
  P("passer", "@star @acme w", 6, 0, { about: false, sentiment: "noise" }),
  P("chatty", "@acme gm", 1), P("chatty", "@acme wagmi", 2), P("chatty", "@acme lfg", 3),
  P("acme", "we shipped v2", 7, 300),
];
const labels = new Map(posts.map((p) => [p.id, p.label]));
const tweets = posts.map(({ label, ...t }) => t);

test("authorFeatures counts posts, days, engagement, inbound reach and label shares", () => {
  const f = authorFeatures(tweets, labels);
  assert.equal(f.promo.n, 4);
  assert.equal(f.promo.days, 4);
  assert.equal(f.promo.ref_r, 0.75);
  assert.equal(f.star.avg_eng, 500);
  assert.equal(f.star.inbound, 1); // only passer mentions @star
  assert.equal(f.hater.dislike_r, 1);
  assert.equal(f.farmer.giveaway_r, 1);
  assert.equal(f.farmer.about_r, 0);
  assert.equal(f.degen.trading_r, 1);
});

test("authorFeatures skips posts with no author instead of crashing", () => {
  const orphan = { id: id(++seq), author: null, text: "@star acme no author here", likes: 5, retweets: 0, replies: 0, quotes: 0, lang: "en", created_at: `Mon Sep  8 10:00:00 +0000 2026` };
  const withOrphan = [...tweets, orphan];
  const withOrphanLabels = new Map([...labels, [orphan.id, { about: true, sentiment: "neutral", topic: "none", feature: "none", interest: null }]]);
  const f = authorFeatures(withOrphan, withOrphanLabels);
  assert.ok(!("null" in f) && !(null in f)); // the orphan post is dropped, no phantom author bucket
  assert.equal(f.promo.n, 4); // real authors are unaffected
});

test("assignRoles gives every author one role by precedence", () => {
  const f = authorFeatures(tweets, labels);
  assignRoles(f, { team: ["acme"] });
  const roles = Object.fromEntries(Object.entries(f).map(([a, x]) => [a, x.role]));
  assert.deepEqual(roles, {
    promo: "promoter", degen: "trader", star: "kol", hater: "critic", farmer: "giveaway",
    passer: "casual", chatty: "other", acme: "official",
  });
});

test("roleStats reports share of authors, posts and engagement per role", () => {
  const f = authorFeatures(tweets, labels);
  assignRoles(f, { team: ["acme"] });
  const s = roleStats(f);
  const sum = (k) => Object.values(s.roles).reduce((a, r) => a + r[k], 0);
  assert.equal(s.totals.authors, 8);
  assert.equal(s.totals.posts, tweets.length);
  assert.ok(Math.abs(sum("pa") - 100) < 0.5 && Math.abs(sum("pp") - 100) < 0.5 && Math.abs(sum("pe") - 100) < 0.5);
  assert.equal(s.roles.kol.pe, Math.round((2500 / 2814) * 1000) / 10);
});

test("dieHards applies the count / days / referral rule and skips the team", () => {
  const f = authorFeatures(tweets, labels);
  assignRoles(f, { team: ["acme"] });
  assert.deepEqual(dieHards(f, { n: 3, days: 3, ref: 0.5 }).map((x) => x.author), ["promo"]);
  assert.deepEqual(dieHards(f, { n: 50, days: 30, ref: 0.15 }), []);
});

test("pickReps takes the most active, the most engaging and the median accounts, once each", () => {
  const f = {};
  for (let i = 1; i <= 20; i++) f["a" + i] = { n: i, avg_eng: 100 - i, role: "other" };
  f.team = { n: 99, avg_eng: 9999, role: "other" };
  const reps = pickReps(f, "other", ["team"]);
  assert.equal(reps.length, 7);
  assert.equal(new Set(reps).size, 7);
  assert.ok(!reps.includes("team"));
  assert.deepEqual(reps.slice(0, 3), ["a20", "a19", "a18"]);
  assert.ok(reps.includes("a1") && reps.includes("a2")); // top avg_eng
});

test("samplePosts keeps the top-engaged posts and spreads the rest across time", () => {
  const many = Array.from({ length: 100 }, (_, i) => ({
    id: String(i), likes: i % 7, retweets: 0, replies: 0, quotes: 0,
    created_at: new Date(Date.UTC(2026, 0, 1 + i)).toUTCString(),
  }));
  const s = samplePosts(many, 35, 20);
  assert.equal(s.length, 35);
  assert.equal(new Set(s.map((p) => p.id)).size, 35);
  const top = many.slice().sort((a, b) => b.likes - a.likes).slice(0, 20).map((p) => p.id);
  for (const t of top) assert.ok(s.some((p) => p.id === t));
  assert.equal(samplePosts(many.slice(0, 10), 35, 20).length, 10);
});
