#!/usr/bin/env node
// ABOUTME: Profiles the accounts behind an app's X mentions: per-author activity, reach and label shares,
// ABOUTME: one role per account, segment shares, the die-hard promoter list and per-role representative files.
//
// Usage: profile-authors.mjs <clean.json> --labels <labels.jsonl> --out <dir> [--team a,b,c]
//        writes <dir>/authors.json, <dir>/role_stats.json, <dir>/reps/<role>.jsonl and prints the tables.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const engOf = (t) => ["likes", "retweets", "replies", "quotes"].reduce((s, k) => s + (Number(t[k]) || 0), 0);
const dayOf = (t) => new Date(t.created_at).toISOString().slice(0, 10);
const r3 = (a, b) => (b ? Math.round((a / b) * 1000) / 1000 : 0);
const REF = new Set(["referral", "paid-promotion"]);
const GIVE_TOPIC = "challenges-giveaways", GIVE_INTEREST = "airdrop-farming", TRADE_TOPIC = "trading-results", TOKEN = "token-team";

export const ROLES = ["official", "giveaway", "promoter", "token-promoter", "kol", "trader", "critic", "casual", "other"];

// One record per author: counts, activity, engagement, inbound reach (distinct other authors that
// @-mention the handle) and the share of their posts per label signal.
export function authorFeatures(tweets, labels) {
  const acc = {}, inbound = {};
  for (const t of tweets) {
    const a = t.author, l = labels.get(t.id) || {};
    if (!a) continue; // a handful of archived posts carry no author; they cannot be attributed
    const r = (acc[a] ||= { n: 0, eng: 0, days: new Set(), first: null, last: null, verified: false,
      about: 0, like: 0, dislike: 0, ref: 0, give: 0, trade: 0, token: 0 });
    const d = dayOf(t);
    r.n++; r.eng += engOf(t); r.days.add(d);
    r.first = r.first && r.first < d ? r.first : d; r.last = r.last && r.last > d ? r.last : d;
    if (t.author_verified) r.verified = true;
    if (l.about) r.about++;
    if (l.sentiment === "like") r.like++;
    if (l.sentiment === "dislike") r.dislike++;
    if (REF.has(l.interest)) r.ref++;
    if (l.topic === GIVE_TOPIC || l.interest === GIVE_INTEREST) r.give++;
    if (l.topic === TRADE_TOPIC) r.trade++;
    if (l.interest === TOKEN) r.token++;
    for (const m of t.text.matchAll(/@(\w{1,15})/g)) {
      const h = m[1].toLowerCase();
      if (h !== a.toLowerCase()) (inbound[h] ||= new Set()).add(a);
    }
  }
  const out = {};
  for (const [a, r] of Object.entries(acc)) {
    out[a] = {
      n: r.n, eng: r.eng, avg_eng: Math.round((r.eng / r.n) * 10) / 10, days: r.days.size,
      span: Math.round((Date.parse(r.last) - Date.parse(r.first)) / 86400000) + 1,
      verified: r.verified, inbound: inbound[a.toLowerCase()]?.size || 0,
      about_r: r3(r.about, r.n), like_r: r3(r.like, r.about), dislike_r: r3(r.dislike, r.about),
      ref_r: r3(r.ref, r.n), giveaway_r: r3(r.give, r.n), trading_r: r3(r.trade, r.n), token_r: r3(r.token, r.n),
    };
  }
  return out;
}

const pct = (xs, p) => { const s = xs.slice().sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.floor(p * s.length))] ?? 0; };

// Tags are independent signals; the role is the first tag in precedence order.
export function assignRoles(feat, { team = [] } = {}) {
  const teamSet = new Set(team.map((h) => h.toLowerCase()));
  const multi = Object.values(feat).filter((f) => f.n >= 5);
  const p95Eng = pct(multi.map((f) => f.avg_eng), 0.95), p95In = pct(multi.map((f) => f.inbound), 0.95);
  for (const [a, f] of Object.entries(feat)) {
    const tags = [];
    if (teamSet.has(a.toLowerCase())) tags.push("official");
    if (f.giveaway_r >= 0.3 || (f.avg_eng === 0 && f.n >= 20)) tags.push("giveaway");
    if (f.ref_r >= 0.2 && f.n >= 3) tags.push("promoter");
    if (f.token_r >= 0.3 && f.n >= 3) tags.push("token-promoter");
    if (f.n >= 5 && ((p95Eng > 0 && f.avg_eng >= p95Eng) || (p95In > 0 && f.inbound >= p95In))) tags.push("kol");
    if (f.trading_r >= 0.4) tags.push("trader");
    if (f.dislike_r >= 0.5 && f.about_r * f.n >= 3) tags.push("critic");
    f.tags = tags;
    f.role = tags[0] || (f.n <= 2 ? "casual" : "other");
  }
  return { p95Eng, p95In };
}

export function roleStats(feat) {
  const totals = { authors: 0, posts: 0, eng: 0 }, roles = {};
  for (const f of Object.values(feat)) {
    totals.authors++; totals.posts += f.n; totals.eng += f.eng;
    const r = (roles[f.role] ||= { authors: 0, posts: 0, eng: 0 });
    r.authors++; r.posts += f.n; r.eng += f.eng;
  }
  const p = (a, b) => Math.round((a / b) * 1000) / 10;
  for (const r of Object.values(roles)) Object.assign(r, { pa: p(r.authors, totals.authors), pp: p(r.posts, totals.posts), pe: p(r.eng, totals.eng) });
  return { totals, roles };
}

// The daily promoters: many posts, on many days, a real share of them carrying a referral.
export function dieHards(feat, { n = 50, days = 30, ref = 0.15 } = {}) {
  return Object.entries(feat)
    .filter(([, f]) => f.role !== "official" && f.n >= n && f.days >= days && f.ref_r >= ref)
    .sort((a, b) => b[1].n - a[1].n)
    .map(([author, f]) => ({ author, ...f }));
}

// Seven accounts per role: the three most active, the two most engaging, two from the middle.
export function pickReps(feat, role, team = [], k = 7) {
  const teamSet = new Set(team.map((h) => h.toLowerCase()));
  const all = Object.entries(feat).filter(([a, f]) => f.role === role && !teamSet.has(a.toLowerCase()));
  const byN = all.slice().sort((a, b) => b[1].n - a[1].n);
  const byEng = all.slice().sort((a, b) => b[1].avg_eng - a[1].avg_eng);
  const mid = Math.floor(all.length / 2);
  const picks = [...byN.slice(0, 3), ...byEng.slice(0, 2), ...byN.slice(mid, mid + 2), ...byN];
  return [...new Set(picks.map(([a]) => a))].slice(0, k);
}

// Up to `cap` posts: the `topK` most engaged, then the rest spread evenly over the account's timeline
// (engagement alone over-represents promotional posts).
export function samplePosts(posts, cap = 35, topK = 20) {
  if (posts.length <= cap) return posts.slice().sort((a, b) => a.created_at.localeCompare(b.created_at));
  const top = posts.slice().sort((a, b) => engOf(b) - engOf(a)).slice(0, topK);
  const chosen = new Set(top.map((p) => p.id));
  const rest = posts.filter((p) => !chosen.has(p.id)).sort((a, b) => Date.parse(a.created_at) - Date.parse(b.created_at));
  const need = cap - top.length, step = rest.length / need;
  for (let i = 0; i < need; i++) chosen.add(rest[Math.floor(i * step)].id);
  return posts.filter((p) => chosen.has(p.id)).sort((a, b) => Date.parse(a.created_at) - Date.parse(b.created_at));
}

function main() {
  const args = process.argv.slice(2);
  const opt = (k) => { const i = args.indexOf(k); return i >= 0 ? args[i + 1] : null; };
  const [cleanPath] = args.filter((a, i) => !a.startsWith("--") && !args[i - 1]?.startsWith("--"));
  const labelsPath = opt("--labels"), out = opt("--out"), team = (opt("--team") || "").split(",").filter(Boolean);
  if (!cleanPath || !labelsPath || !out) {
    console.error("usage: profile-authors.mjs <clean.json> --labels <labels.jsonl> --out <dir> [--team a,b]");
    process.exit(1);
  }
  const tweets = JSON.parse(readFileSync(cleanPath, "utf8"));
  const labels = new Map(readFileSync(labelsPath, "utf8").split("\n").filter((l) => l.trim()).map((l) => { const x = JSON.parse(l); return [x.id, x]; }));
  const unlabeled = tweets.filter((t) => !labels.has(t.id)).length;
  const feat = authorFeatures(tweets, labels);
  const thresholds = assignRoles(feat, { team });
  const stats = roleStats(feat);
  mkdirSync(join(out, "reps"), { recursive: true });
  writeFileSync(join(out, "authors.json"), JSON.stringify(feat));
  writeFileSync(join(out, "role_stats.json"), JSON.stringify({ ...stats, thresholds, unlabeled }, null, 1));

  console.log(`posts ${tweets.length} (${unlabeled} without a label), authors ${stats.totals.authors}, engagement ${stats.totals.eng}`);
  console.log(`kol thresholds (P95 of n>=5): avg_eng ${thresholds.p95Eng}, inbound ${thresholds.p95In}`);
  const once = Object.values(feat).filter((f) => f.n === 1).length;
  console.log(`authors with one post: ${once} (${Math.round((once / stats.totals.authors) * 1000) / 10}%)\n`);
  console.log("role            authors  %authors  %posts  %eng");
  for (const r of ROLES) {
    const s = stats.roles[r]; if (!s) continue;
    console.log(`${r.padEnd(15)} ${String(s.authors).padStart(7)}  ${String(s.pa).padStart(8)}  ${String(s.pp).padStart(6)}  ${String(s.pe).padStart(4)}`);
  }
  const byAuthor = {};
  for (const t of tweets) (byAuthor[t.author] ||= []).push(t);
  console.log("\ndie-hards (n>=50, days>=30, ref_r>=0.15): author n days ref_r inbound role");
  for (const d of dieHards(feat)) console.log(`  @${d.author} ${d.n} ${d.days} ${d.ref_r} ${d.inbound} ${d.role}`);
  for (const [a, f] of Object.entries(feat)) if (f.role === "official") console.log(`official @${a}: ${f.n} posts, ${f.days} days, mentioned by ${f.inbound} accounts`);

  console.log("\nrepresentatives:");
  for (const role of ROLES) {
    if (role === "official") continue;
    const reps = pickReps(feat, role, team);
    if (!reps.length) continue;
    const lines = reps.map((a) => JSON.stringify({
      account: a, role, features: feat[a],
      posts: samplePosts(byAuthor[a]).map((t) => ({
        date: dayOf(t), eng: engOf(t), lang: t.lang, text: t.text, url: `https://x.com/${t.author}/status/${t.id}`,
        label: labels.get(t.id) ? { sentiment: labels.get(t.id).sentiment, topic: labels.get(t.id).topic, interest: labels.get(t.id).interest } : null,
      })),
    }));
    writeFileSync(join(out, "reps", `${role}.jsonl`), lines.join("\n") + "\n");
    console.log(`  ${role}: ${reps.map((a) => `@${a}(${feat[a].n})`).join(" ")}`);
  }
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
