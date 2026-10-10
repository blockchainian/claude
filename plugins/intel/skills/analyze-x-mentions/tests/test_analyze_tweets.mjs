// ABOUTME: Tests the analyze-x-mentions scripts on a tiny fixture: cleaning rules and date range,
// ABOUTME: topic counts and timeline, chunking, and the summary id/quote verification.
import { test } from "node:test";
import assert from "node:assert/strict";
import { clean, facts, readLog, botAuthors } from "../scripts/clean.mjs";
import { writeFileSync, mkdtempSync, readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chunk } from "../scripts/chunk.mjs";
import { verifySummary, tally, byFeature, byTopic, topTexts, norm, readLabels, inWindow, timeline } from "../scripts/aggregate.mjs";
import { mergeLabels, growVocab, applyAliases } from "../scripts/merge-labels.mjs";
import { splitPosts, runPool } from "../scripts/run-labels.mjs";

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
  assert.deepEqual(dropped, { outOfRange: 0, bot: 1, massTag: 1, dupe: 1, short: 1, botAccount: 0 });
  const ranged = clean(fixture, undefined, "2026-09-04", "2026-09-05").clean;
  assert.deepEqual(ranged.map((t) => t.id), [id(5)]);
  const f = facts(fixture, c);
  assert.equal(f.raw, 7); assert.equal(f.clean, 3);
  assert.equal(f.from, "2026-09-02"); assert.equal(f.to, "2026-09-05");
});

const V = (n, author, text, day, likes = 0, views = 0) => ({ ...T(n, author, text, day, likes), views: String(views) });
test("clean drops fake/automated accounts (high volume, no real impressions) but keeps distributed or replied-to accounts", () => {
  // shillbot: 60 unique posts, near-zero views, few likes, nobody @-mentions it back -> low reach
  const bots = [];
  for (let i = 0; i < 60; i++) bots.push(V(1000 + i, "shillbot", `\$COIN${i} is live on acme, use my ref link`, 2, 1, 120));
  // deadfollower: high volume + real inbound, but a median post is seen by almost nobody -> dead reach (the @qkl2058 shape)
  const dead = [];
  for (let i = 0; i < 60; i++) dead.push(V(4000 + i, "deadfollower", `gm acme community update ${i}`, 2, 0, 40));
  // realkol: modest views but genuine likes and lots of distinct repliers -> kept
  const humans = [];
  for (let i = 0; i < 60; i++) humans.push(V(2000 + i, "realkol", `acme thread part ${i}, my honest take`, 2, 40, 1800));
  const backat = [];
  for (let i = 0; i < 20; i++) backat.push(V(3000 + i, "fan" + i, "@realkol @deadfollower gm", 3, 1, 500));
  const all = [...bots, ...dead, ...humans, ...backat];
  const kept = new Set(clean(all).clean.map((t) => t.author));
  assert.ok(!kept.has("shillbot"), "no-impression high-volume account is dropped");
  assert.ok(!kept.has("deadfollower"), "high-volume account with dead impressions is dropped even with some inbound");
  assert.ok(kept.has("realkol"), "the distributed, replied-to account is kept");
  assert.deepEqual([...botAuthors(all)].sort(), ["deadfollower", "shillbot"]);
  assert.equal(clean(all, undefined, null, null, false).dropped.botAccount, 0); // --keep-bots off
});

test("botAuthors with follower counts applies the demoted / automated / suspended rules", () => {
  const posts = [];
  // demoted: big following but a median post reaches almost none of them (X promoted it down)
  for (let i = 0; i < 60; i++) posts.push(V(5000 + i, "demoted", `acme take ${i}`, 2, 20, 300));
  // automated: few followers, posts far more than anyone follows
  for (let i = 0; i < 60; i++) posts.push(V(6000 + i, "autobot", `$X${i} live on acme`, 2, 20, 300));
  // realbig: big following AND a median post pulls thousands of views -> kept
  for (let i = 0; i < 60; i++) posts.push(V(7000 + i, "realbig", `acme analysis ${i}`, 2, 50, 6000));
  const followers = {
    demoted: { followers: 100000, tweets: 40000, status: 200 },
    autobot: { followers: 200, tweets: 5000, status: 200 },
    realbig: { followers: 100000, tweets: 20000, status: 200 },
  };
  const bots = botAuthors(posts, followers);
  assert.ok(bots.has("demoted"), "many followers + few views = demoted/fake");
  assert.ok(bots.has("autobot"), "few followers + huge post count = automated");
  assert.ok(!bots.has("realbig"), "big following with real per-post reach is kept");
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

test("label-codex builds one prompt per chunk and validates the model's labels against it", async () => {
  const { buildPrompt, parseLabels, codexHome } = await import("../scripts/label-codex.mjs");
  const { SCHEMA } = await import("../scripts/mentions-spec.mjs");
  const posts = chunk(fixture, 10)[0];
  const prompt = buildPrompt("APP FACTS", posts, { topics: ["fees-pricing"], features: ["ui"] });
  assert.ok(prompt.startsWith("APP FACTS"));
  assert.ok(prompt.includes("Examples: fees-pricing."));
  assert.ok(prompt.includes("Examples: ui."));
  assert.ok(prompt.includes("Examples: none yet"));
  assert.ok(prompt.includes(`1\talice\t50\tSep 2 \ten\tacme app is the cleanest UI in crypto`));
  assert.equal(SCHEMA.properties.labels.items.required.length, 8);
  const dir = mkdtempSync(join(tmpdir(), "codex-home-"));
  process.env.CODEX_HOME = dir;
  writeFileSync(join(dir, "auth.json"), "{}");
  writeFileSync(join(dir, "AGENTS.md"), "say hi");
  const home = codexHome(dir, { model: "m", effort: "low", instructions: "/x/rules.md" });
  assert.deepEqual(readdirSync(home).sort(), ["auth.json", "config.toml"]);
  assert.match(readFileSync(join(home, "config.toml"), "utf8"), /model = "m"\nmodel_reasoning_effort = "low"\nmodel_instructions_file = "\/x\/rules.md"\nproject_doc_max_bytes = 0/);
  const good = posts.map((p, i) => ({ n: i + 1, about: false, sentiment: "noise", topic: "none", feature: "none", point: "", request: null, interest: null }));
  const want = posts.map((p, i) => ({ id: p.id, ...good[i], n: undefined }));
  assert.deepEqual(parseLabels(JSON.stringify({ labels: good }), posts), want.map(({ n, ...l }) => l));
  assert.throws(() => parseLabels(JSON.stringify({ labels: good.slice(1) }), posts), /missing 1 posts/);
  assert.throws(() => parseLabels(JSON.stringify({ labels: [good[1], good[0], ...good.slice(2)] }), posts), /order/);
  assert.throws(() => parseLabels(JSON.stringify({ labels: [...good, { ...good[0], n: 99 }] }), posts), /unknown/);
});

test("label-codex gap-fills only the posts the model dropped, and never writes a partial chunk", async () => {
  const { collectLabels, labelWithRetry } = await import("../scripts/label-codex.mjs");
  const posts = chunk(fixture, 10)[0];
  const mk = (n) => ({ n, about: false, sentiment: "noise", topic: "none", feature: "none", point: "", request: null, interest: null });

  // collectLabels is lenient: first-seen wins, out-of-range and un-parseable input drop to nothing, never throws.
  const m = collectLabels(JSON.stringify({ labels: [mk(2), { ...mk(2), point: "dup" }, mk(99), mk(1)] }), posts.length);
  assert.deepEqual([...m.keys()].sort((a, b) => a - b), [1, 2]);
  assert.equal(m.get(2).point, "");
  assert.equal(collectLabels("not json", posts.length).size, 0);

  // A labeler that returns only the first post on pass 1, then the rest: the second pass asks for exactly the gap.
  let pass = 0;
  const flaky = (sub) => {
    pass++;
    const take = pass === 1 ? 1 : sub.length;
    return { message: JSON.stringify({ labels: sub.slice(0, take).map((_, i) => mk(i + 1)) }), usage: { input_tokens: 10, output_tokens: 5 }, seconds: 1, tools: 0 };
  };
  const { labels, passes } = labelWithRetry(posts, flaky, { maxPasses: 3 });
  assert.equal(labels.length, posts.length);
  assert.deepEqual(labels.map((l) => l.id), posts.map((p) => p.id));
  assert.ok(labels.every((l) => !("n" in l)));
  assert.equal(passes.length, 2);
  assert.equal(passes[1].asked, posts.length - 1);

  // A labeler that always drops the last post throws rather than writing a short chunk.
  const stuck = (sub) => ({ message: JSON.stringify({ labels: sub.slice(0, -1).map((_, i) => mk(i + 1)) }), usage: {}, seconds: 1, tools: 0 });
  assert.throws(() => labelWithRetry(posts, stuck, { maxPasses: 3 }), /missing 1 of \d+ posts after 3 passes/);

  // A labeler that throws on the first pass still recovers on the next.
  let crash = 0;
  const crashy = (sub) => { crash++; if (crash === 1) throw new Error("boom"); return { message: JSON.stringify({ labels: sub.map((_, i) => mk(i + 1)) }), usage: {}, seconds: 1, tools: 0 }; };
  assert.equal(labelWithRetry(posts, crashy, { maxPasses: 3 }).labels.length, posts.length);
});

test("splitPosts divides into roughly-even parts and never loses a post", () => {
  const posts = Array.from({ length: 1500 }, (_, i) => ({ id: id(i), text: "x" }));
  const parts = splitPosts(posts, 3);
  assert.equal(parts.length, 3);
  assert.deepEqual(parts.map((p) => p.length), [500, 500, 500]);
  assert.equal(parts.flat().length, 1500);
  // uneven split keeps every post, no empty tail
  const p167 = splitPosts(Array.from({ length: 167 }, (_, i) => ({ id: i })), 3);
  assert.equal(p167.reduce((s, p) => s + p.length, 0), 167);
  assert.ok(p167.every((p) => p.length > 0));
  // indivisible input returns itself
  assert.deepEqual(splitPosts([{ id: 1 }], 3).length, 1);
});

test("runPool splits failures, recurses to the depth cap, and reports leftovers", async () => {
  // A unit "succeeds" only once it is small enough (<= 200), modeling early-stop on big chunks.
  let maxActive = 0, active = 0;
  const runOne = async (u) => {
    active++; maxActive = Math.max(maxActive, active);
    await new Promise((r) => setTimeout(r, 1));
    active--;
    return u.size <= 200;
  };
  const onSplit = (u) => {
    const size = Math.ceil(u.size / 3);
    return [0, 1, 2].map(() => ({ size, depth: u.depth + 1 }));
  };
  // 1500 -> 500 -> 167(ok). Two levels of splitting; everything resolves.
  const okRun = await runPool([{ size: 1500, depth: 0 }], { par: 4, maxDepth: 2, runOne, onSplit });
  assert.deepEqual(okRun.failed, []);
  assert.ok(maxActive <= 4, "never exceeds the parallel cap");

  // If even leaf units cannot pass, they are recorded as failed at the depth cap, not retried forever.
  const neverOk = async () => false;
  const bad = await runPool([{ size: 1500, depth: 0 }], { par: 4, maxDepth: 2, runOne: neverOk, onSplit });
  assert.equal(bad.failed.length, 9); // 3 x 3 leaves at depth 2
  assert.ok(bad.failed.every((u) => u.depth === 2));
});

test("labelWithRetry reports each pass via onPass so a short answer is visible", async () => {
  const { labelWithRetry } = await import("../scripts/label-codex.mjs");
  const posts = Array.from({ length: 5 }, (_, i) => ({ id: id(i) }));
  const lbl = (ns) => ({ message: JSON.stringify({ labels: ns.map((n) => ({ n, about: false, sentiment: "noise", topic: "none", feature: "none", point: null, request: null, interest: null })) }) });
  const returns = [lbl([1, 2]), lbl([1, 2, 3])]; // pass1: asked 5, got 2 (short); pass2: asked 3, got 3
  let i = 0;
  const events = [];
  const { labels } = labelWithRetry(posts, () => returns[i++], { maxPasses: 3, onPass: (e) => events.push(e) });
  assert.equal(labels.length, 5);
  assert.deepEqual(events.map((e) => [e.pass, e.asked, e.got, e.remaining]), [[1, 5, 2, 3], [2, 3, 3, 0]]);
});

test("label-codex takes its rules, fields and schema from a spec, defaulting to the mentions spec", async () => {
  const { buildPrompt, parseLabels } = await import("../scripts/label-codex.mjs");
  const mentions = await import("../scripts/mentions-spec.mjs");
  const posts = chunk(fixture, 10)[0];
  assert.equal(mentions.FIELDS[0], "id");
  assert.deepEqual(mentions.SCHEMA.properties.labels.items.required, ["n", ...mentions.FIELDS.slice(1)]);
  assert.ok(mentions.isNoise({ sentiment: "noise" }));
  assert.deepEqual(mentions.VOCAB, { topic: "topics", feature: "features", interest: "interests" });
  const spec = {
    FIELDS: ["id", "about", "kind"],
    fields: (vocab) => `FIELDS kinds: ${(vocab.kinds || []).join(", ")}`,
  };
  const prompt = buildPrompt("FACTS", posts, { kinds: ["call"] }, spec);
  assert.ok(prompt.startsWith("FACTS\nFIELDS kinds: call\n"));
  const good = posts.map((p, i) => ({ n: i + 1, about: true, kind: "call", extra: "dropped" }));
  assert.deepEqual(parseLabels(JSON.stringify({ labels: good }), posts, spec), posts.map((p) => ({ id: p.id, about: true, kind: "call" })));
});

test("growVocab grows the lists a spec names, not only the mentions fields", () => {
  const rows = [{ about: true, topic: "market-macro", asset: "BTC", interest: "own-token" }, { about: true, topic: "market-macro", asset: "BTC", interest: null }];
  const vocab = { topics: [] };
  const g = growVocab(rows, vocab, 0.5, { topic: "topics", interest: "interests" });
  assert.deepEqual(vocab, { topics: ["market-macro"], interests: ["own-token"] });
  assert.deepEqual(g.added, { topics: [["market-macro", 2]], interests: [["own-token", 1]] });
  assert.ok(!("assets" in vocab));
});

test("render_charts puts a relative out_dir under the intel output root", () => {
  const root = mkdtempSync(join(tmpdir(), "out-"));
  const cwd = mkdtempSync(join(tmpdir(), "cwd-"));
  const script = fileURLToPath(new URL("../scripts/render_charts.py", import.meta.url));
  const r = spawnSync(script, [], { cwd, input: JSON.stringify({ out_dir: "x/acme/images", charts: [] }), env: { ...process.env, INTEL_OUTPUT_DIR: root }, encoding: "utf8" });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(existsSync(join(root, "x", "acme", "images")));
  assert.ok(!existsSync(join(cwd, "x")));
});
