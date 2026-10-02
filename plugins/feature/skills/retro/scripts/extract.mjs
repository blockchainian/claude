#!/usr/bin/env node
// ABOUTME: Objective retro evidence for a named Claude Code session — spawn ledger + token-share-by-role.
// ABOUTME: Joins orchestrator Task spawns to subagents/<agent>.meta.json (toolUseId) and sums each child's tokens.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { StringDecoder } from "node:string_decoder";
import { parseArgs } from "node:util";

const DEFAULT_ROOT = path.join(os.homedir(), ".claude", "projects");
const DEFAULT_CODEX_ROOT = path.join(os.homedir(), ".codex", "sessions");
const BASES = ["output", "billable", "full"];
const PROG = "extract.mjs";
const USAGE =
  `usage: ${PROG} [-h] [--root ROOT] [--basis {output,billable,full}]\n` +
  "                   [--codex-root CODEX_ROOT]\n" +
  "                   names [names ...]\n";
const HELP =
  USAGE +
  "\nObjective retro evidence for a named Claude Code session.\n" +
  "\npositional arguments:\n" +
  "  names                 session title(s), as set by /rename\n" +
  "\noptions:\n" +
  "  -h, --help            show this help message and exit\n" +
  "  --root ROOT           projects dir (default ~/.claude/projects)\n" +
  "  --basis {output,billable,full}\n" +
  "                        token counting basis (default billable =\n" +
  "                        input+cache_creation+output)\n" +
  "  --codex-root CODEX_ROOT\n" +
  "                        codex sessions dir (default ~/.codex/sessions)\n";

// ---- small helpers that reproduce the Python formatting the output contract depends on ----

const isObj = (v) => v !== null && typeof v === "object" && !Array.isArray(v);

/** Python str(): None for null/undefined, True/False for booleans. */
function pyStr(v) {
  if (v == null) return "None";
  if (typeof v === "boolean") return v ? "True" : "False";
  return String(v);
}

/** Python repr() for the scalar kinds that reach the output (strings pick the quote the way Python does). */
function pyRepr(v) {
  if (typeof v !== "string") return pyStr(v);
  const q = v.includes("'") && !v.includes('"') ? '"' : "'";
  let out = "";
  for (const ch of v) {
    if (ch === "\\") out += "\\\\";
    else if (ch === q) out += "\\" + q;
    else if (ch === "\n") out += "\\n";
    else if (ch === "\r") out += "\\r";
    else if (ch === "\t") out += "\\t";
    else {
      const c = ch.codePointAt(0);
      out += c < 0x20 || c === 0x7f ? "\\x" + c.toString(16).padStart(2, "0") : ch;
    }
  }
  return q + out + q;
}

/** Python len() on a str counts code points, not UTF-16 units. */
function pyLen(s) {
  let n = s.length;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const d = s.charCodeAt(i + 1);
      if (d >= 0xdc00 && d <= 0xdfff) { n--; i++; }
    }
  }
  return n;
}

/** Python format(n, ",") — thousands separators. */
function fmtComma(n) {
  const s = String(n);
  const neg = s.startsWith("-");
  const [intPart, frac] = (neg ? s.slice(1) : s).split(".");
  const grouped = intPart.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return (neg ? "-" : "") + grouped + (frac !== undefined ? "." + frac : "");
}

/** Python format(x, `.${d}f`): rounds the exact binary value half-to-even, unlike toFixed (half-up). */
function pyFixed(x, d) {
  if (Number.isNaN(x)) return "nan";
  if (!Number.isFinite(x)) return x > 0 ? "inf" : "-inf";
  const neg = x < 0 || Object.is(x, -0);
  const exact = Math.abs(x).toFixed(Math.min(100, d + 60));
  const [intPart, frac = ""] = exact.split(".");
  let digits = intPart + frac.slice(0, d);
  const rest = frac.slice(d);
  let up = false;
  if (rest.length) {
    const head = rest[0];
    const tail = rest.slice(1);
    if (head > "5") up = true;
    else if (head === "5") up = /[1-9]/.test(tail) || (Number(digits[digits.length - 1]) % 2 === 1);
  }
  if (up) {
    const arr = digits.split("");
    let i = arr.length - 1;
    while (i >= 0) {
      if (arr[i] === "9") { arr[i] = "0"; i--; } else { arr[i] = String(Number(arr[i]) + 1); break; }
    }
    if (i < 0) arr.unshift("1");
    digits = arr.join("");
  }
  const ip = d ? digits.slice(0, digits.length - d) : digits;
  const fp = d ? digits.slice(digits.length - d) : "";
  return (neg ? "-" : "") + (ip || "0") + (d ? "." + fp : "");
}

/** Python tuple ordering for the (number, string, ...) row tuples the report sorts with reverse=True. */
function compareTuples(a, b) {
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i], y = b[i];
    if (typeof x === "number" && typeof y === "number") {
      if (x !== y) return x < y ? -1 : 1;
      continue;
    }
    const xs = pyStr(x), ys = pyStr(y);
    if (xs !== ys) return xs < ys ? -1 : 1;
  }
  return 0;
}

/** Python Counter.most_common(1)[0][0]: highest count, first-inserted on ties; null when empty. */
function mostCommon(counter) {
  let best = null, bestCount = -Infinity;
  for (const [k, v] of counter) if (v > bestCount) { best = k; bestCount = v; }
  return best;
}

function increment(counter, key, by = 1) {
  counter.set(key, (counter.get(key) ?? 0) + by);
}

/** Streams a file line by line (trailing newline kept, like Python's file iteration) without loading it whole. */
function* readLines(filePath) {
  const fd = fs.openSync(filePath, "r");
  try {
    const decoder = new StringDecoder("utf8");
    const buf = Buffer.allocUnsafe(1 << 20);
    let rest = "";
    for (;;) {
      const n = fs.readSync(fd, buf, 0, buf.length, null);
      if (n === 0) break;
      const parts = (rest + decoder.write(buf.subarray(0, n))).split("\n");
      rest = parts.pop();
      for (const p of parts) yield p + "\n";
    }
    rest += decoder.end();
    if (rest) yield rest;
  } finally {
    fs.closeSync(fd);
  }
}

/** Each line parsed as JSON; unparseable lines and non-object records are skipped. */
function* readRecords(filePath) {
  for (const line of readLines(filePath)) {
    let o;
    try { o = JSON.parse(line); } catch { continue; }
    if (isObj(o)) yield [o, line];
  }
}

function isDir(p) {
  try { return fs.statSync(p).isDirectory(); } catch { return false; }
}

/** Python glob's recursive `**` walk: every file under root matching `pred`, hidden dirs/files skipped, symlinked dirs followed. */
function walkFiles(root, pred, out = []) {
  if (!isDir(root)) return out;
  let entries;
  try { entries = fs.readdirSync(root, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    if (e.name.startsWith(".")) continue;
    const full = path.join(root, e.name);
    if (e.isDirectory() || (e.isSymbolicLink() && isDir(full))) walkFiles(full, pred, out);
    else if (pred(e.name)) out.push(full);
  }
  return out;
}

// ---- transcript readers ----

/** Session name -> its top-level transcript path(s), via the customTitle record /rename writes. */
function resolve(name, root) {
  const res = spawnSync("grep", ["-rl", `"customTitle":"${name}"`, "--include=*.jsonl", root], { encoding: "utf8" });
  const out = (res.stdout ?? "").split(/\s+/).filter(Boolean);
  return out.filter((p) => !p.includes("/subagents/"));
}

function tok(u, basis) {
  const i = u.input_tokens ?? 0, cc = u.cache_creation_input_tokens ?? 0;
  const o = u.output_tokens ?? 0, cr = u.cache_read_input_tokens ?? 0;
  return { output: o, billable: i + cc + o, full: i + cc + o + cr }[basis];
}

/**
 * Return [tokens, turns, primaryModel] for one transcript, model taken from its own turns.
 *
 * Dedupes by message.id: the harness can write one assistant response across several JSONL
 * lines that each repeat the same message.usage, so summing per line inflates the total
 * (measured ~3x on a real session). Lines without an id cannot be deduped and are each counted.
 */
function fileStats(filePath, basis) {
  let tokens = 0, turns = 0;
  const models = new Map();
  const seen = new Set();
  for (const [o] of readRecords(filePath)) {
    if (o.type !== "assistant") continue;
    const m = isObj(o.message) ? o.message : {};
    const mid = m.id ?? null;
    if (mid !== null) {
      if (seen.has(mid)) continue;
      seen.add(mid);
    }
    increment(models, m.model ?? null);
    turns += 1;
    tokens += tok(isObj(m.usage) ? m.usage : {}, basis);
  }
  const primary = models.size ? mostCommon(models) : "?";
  return [tokens, turns, primary];
}

/** tool_use id -> {subagent_type, desc} for every Task/Agent spawn in the orchestrator transcript. */
function spawns(orch) {
  const out = new Map();
  for (const [o] of readRecords(orch)) {
    const m = o.message;
    const content = isObj(m) ? m.content : null;
    if (!Array.isArray(content)) continue;
    for (const b of content) {
      if (isObj(b) && b.type === "tool_use" && (b.name === "Task" || b.name === "Agent")) {
        const inp = isObj(b.input) ? b.input : {};
        out.set(b.id ?? null, { subagent_type: inp.subagent_type ?? null, desc: inp.description ?? null });
      }
    }
  }
  return out;
}

function hasCodexLane(sp) {
  for (const v of sp.values()) if ((v.subagent_type || "").startsWith("codex:")) return true;
  return false;
}

function parseTs(value) {
  const t = Date.parse(String(value).replace("Z", "+00:00"));
  return Number.isNaN(t) ? null : t / 1000;
}

/**
 * Profile the orchestrator's OWN context: compaction boundaries, peak context, and what filled it.
 *
 * A monolithic session that rides the context ceiling and auto-compacts is a topology waste the
 * per-role token totals hide (compaction shrinks the prefix, so it leaves no spike). It also names
 * the content driving the climb — images (screenshots/composites read into the main loop) cost tens
 * of thousands of tokens each and dwarf text, and the same file re-read N times is pure waste.
 * Returns an object; prints nothing.
 */
function contextHealth(orch) {
  const nameOf = new Map(), pathOf = new Map();
  const compactions = [];
  let peakCtx = 0, peakTs = null;
  let ccSum = 0, outSum = 0;
  let imgReads = 0, imgB64 = 0, txtResult = 0;
  const reads = new Map();
  const seen = new Set();
  for (const [o] of readRecords(orch)) {
    if (o.subtype === "compact_boundary" || o.isCompactSummary) compactions.push(o.timestamp ?? null);
    const m = o.message;
    const content = isObj(m) ? m.content : null;
    if (o.type === "assistant" && isObj(m)) {
      const u = isObj(m.usage) ? m.usage : {};
      const cr = u.cache_read_input_tokens ?? 0;
      if (cr > peakCtx) { peakCtx = cr; peakTs = o.timestamp ?? null; }
      const mid = m.id ?? null;
      if (mid === null || !seen.has(mid)) {
        if (mid !== null) seen.add(mid);
        ccSum += u.cache_creation_input_tokens ?? 0;
        outSum += u.output_tokens ?? 0;
      }
    }
    if (!Array.isArray(content)) continue;
    for (const b of content) {
      if (!isObj(b)) continue;
      if (b.type === "tool_use" && b.name === "Read") {
        nameOf.set(b.id ?? null, "Read");
        const inp = isObj(b.input) ? b.input : {};
        pathOf.set(b.id ?? null, pyStr(inp.file_path ?? "").split("/").pop());
      }
      if (b.type === "tool_result") {
        const cont = b.content ?? "";
        if (!Array.isArray(cont)) continue;
        for (const x of cont) {
          if (!isObj(x)) continue;
          if (x.type === "image") {
            imgReads += 1;
            const src = x.source ?? {};
            imgB64 += isObj(src) && typeof src.data === "string" ? pyLen(src.data) : 0;
            const tid = b.tool_use_id ?? null;
            if (nameOf.get(tid) === "Read") increment(reads, pathOf.has(tid) ? pathOf.get(tid) : "?");
          } else if (x.type === "text") {
            txtResult += typeof x.text === "string" ? pyLen(x.text) : 0;
          }
        }
      }
    }
  }
  const reread = {};
  for (const [k, v] of reads) if (v > 1) reread[k] = v;
  return {
    // one compaction writes both a compact_boundary and an isCompactSummary record, with
    // sub-second-different stamps — dedup to second precision
    compactions: [...new Set(compactions.filter((c) => c).map((c) => pyStr(c).slice(0, 19)))].sort(),
    peak_ctx: peakCtx, peak_ts: peakTs,
    cc_sum: ccSum, out_sum: outSum,
    img_reads: imgReads, img_b64: imgB64, txt_result_tok: Math.floor(txtResult / 4),
    reread,
  };
}

/** cwd, [minTs, maxTs], and any codex thread ids recorded in the orchestrator transcript. */
function orchContext(orch) {
  const cwds = new Map();
  let lo = null, hi = null;
  const threads = new Set();
  for (const [o, line] of readRecords(orch)) {
    if (o.cwd) increment(cwds, o.cwd);
    const ts = parseTs(o.timestamp);
    if (ts !== null) {
      lo = lo === null ? ts : Math.min(lo, ts);
      hi = hi === null ? ts : Math.max(hi, ts);
    }
    // codex-manager tool results carry "threadId": "<id>" — inside a tool_result
    // string, so unescape one level of JSON quoting first.
    const unescaped = line.replaceAll('\\"', '"');
    for (const m of unescaped.matchAll(/"threadId"\s*:\s*"([^"]+)"/g)) threads.add(m[1]);
  }
  const cwd = cwds.size ? mostCommon(cwds) : null;
  return [cwd, lo, hi, threads];
}

function codexMeta(filePath) {
  let n = 0;
  for (const line of readLines(filePath)) {
    if (++n > 5) break;
    let o;
    try { o = JSON.parse(line); } catch { continue; }
    if (isObj(o) && o.type === "session_meta") return "payload" in o ? o.payload : o;
  }
  return null;
}

/** Last cumulative thread_token_usage in the rollout, on the requested basis. */
function codexTokens(filePath, basis) {
  let u = {};
  for (const line of readLines(filePath)) {
    if (!line.includes("token_usage_record")) continue;
    let o;
    try { o = JSON.parse(line); } catch { continue; }
    if (!isObj(o)) continue;
    const payload = ("payload" in o ? o.payload : o) || {};
    const tt = isObj(payload) ? payload.thread_token_usage : undefined;
    if (isObj(tt) && tt.total_tokens != null) u = tt;
  }
  const i = u.input_tokens ?? 0, cw = u.cache_write_input_tokens ?? 0;
  const out = u.output_tokens ?? 0, rsn = u.reasoning_output_tokens ?? 0;
  return { output: out + rsn, billable: i + cw + out + rsn, full: u.total_tokens ?? 0 }[basis];
}

/**
 * Join the codex lane's real token cost: by recorded thread id (deterministic),
 * else by originator + cwd + time window (correlation). Returns [rows, deterministicBool].
 */
function codexCost(orch, codexRoot, basis) {
  const [cwd, lo, hi, threads] = orchContext(orch);
  const rows = [];
  // recursive ** matches rollouts in date subdirs and directly under codex_root
  const rollouts = [...new Set(walkFiles(codexRoot, (n) => n.startsWith("rollout-") && n.endsWith(".jsonl")))].sort();
  for (const p of rollouts) {
    const meta = codexMeta(p);
    if (!meta || !isObj(meta) || !Object.keys(meta).length) continue;
    const sid = meta.id || meta.session_id || null;
    const det = threads.has(sid);
    const ts = parseTs(meta.timestamp);
    const corr = meta.originator === "Claude Code" && Boolean(cwd) && meta.cwd === cwd &&
      ts !== null && lo !== null && hi !== null && lo <= ts && ts <= hi;
    if (det || corr) rows.push([codexTokens(p, basis), sid, det ? "exact" : "corr", path.basename(p)]);
  }
  rows.sort((a, b) => compareTuples(b, a));
  return [rows, threads.size > 0];
}

// ---- report ----

function report(name, root, basis, codexRoot) {
  console.log(`\n==== ${name} ====`);
  const paths = resolve(name, root);
  if (!paths.length) {
    console.log(`  !! no session titled ${pyRepr(name)} under ${root}`);
    return;
  }
  const orch = paths[0];
  console.log(`  orchestrator transcript: ${orch}`);
  if (paths.length > 1) {
    console.log(`  (note: ${paths.length} transcripts carry this title; using the first — grep the rest by hand)`);
  }

  const [ot, oturns, omodel] = fileStats(orch, basis);
  console.log(`  orchestrator own cost: ${fmtComma(ot)} tok (${basis}), ${oturns} turns, ${pyStr(omodel)}`);

  const h = contextHealth(orch);
  const ncomp = h.compactions.length;
  console.log(`  orchestrator context: peak ${fmtComma(h.peak_ctx)} tok` +
    (h.peak_ts ? ` @${pyStr(h.peak_ts).slice(0, 19)}` : "") +
    `; ${ncomp} auto-compaction(s)` +
    (ncomp ? ` at ${h.compactions.map((c) => pyStr(c).slice(0, 19)).join(", ")}` : "") +
    `; cache_creation ${fmtComma(h.cc_sum)} vs output ${fmtComma(h.out_sum)}`);
  console.log(`  orchestrator content: ${h.img_reads} image read(s) into main loop ` +
    `(base64 ${fmtComma(h.img_b64)} chars; images cost ~tens-of-k tok each, dwarf text — ` +
    `text tool_results only ~${fmtComma(h.txt_result_tok)} tok)`);
  const rereadEntries = Object.entries(h.reread);
  if (rereadEntries.length) {
    const top = rereadEntries.sort((a, b) => b[1] - a[1]).slice(0, 6);
    console.log("  ** redundant image re-reads (same file, N×): " +
      top.map(([f, n]) => `${n}× ${f}`).join(", ") +
      " — each re-read re-adds the whole image to context **");
  }
  if (ncomp || h.peak_ctx > 700_000) {
    console.log("  ** context rode the ceiling: rank as a topology waste. Images never evict from a live " +
      "context, so loading many large ones into one session IS what forces compaction — a " +
      "capacity mechanic, not a judging one; compare images in bulk with a script. Compaction " +
      "hides in the token totals (it SHRINKS the prefix). **");
  }

  const sp = spawns(orch);
  const ledger = new Map();
  for (const v of sp.values()) increment(ledger, v.subagent_type);
  const ledgerRepr = "{" + [...ledger].map(([k, v]) => `${pyRepr(k)}: ${v}`).join(", ") + "}";
  console.log(`  spawn ledger: ${ledgerRepr}`);

  const sdir = orch.slice(0, -".jsonl".length) + "/subagents";
  console.log(`  subagents dir: ${sdir}`);
  let total = 0;
  if (!isDir(sdir)) {
    console.log("  NO subagents/ dir — no Claude subagent cost recorded for this session.");
  } else {
    const rows = [];
    const byModel = new Map();
    const metaFiles = fs.readdirSync(sdir).filter((n) => n.endsWith(".meta.json") && !n.startsWith("."));
    for (const metaName of metaFiles) {
      const metaPath = path.join(sdir, metaName);
      let meta;
      try { meta = JSON.parse(fs.readFileSync(metaPath, "utf8")); } catch { continue; }
      if (!isObj(meta)) meta = {};
      const jpath = metaPath.slice(0, -".meta.json".length) + ".jsonl";
      if (!fs.existsSync(jpath)) continue;
      const [tk, turns, model] = fileStats(jpath, basis);
      const label = (sp.get(meta.toolUseId ?? null) || {}).desc || meta.description || "?";
      rows.push([tk, model, meta.agentType ?? null, turns, label]);
      increment(byModel, model, tk);
      total += tk;
    }
    rows.sort((a, b) => compareTuples(b, a));
    console.log(`  --- subagents (${rows.length}), ranked by ${basis} tokens ---`);
    for (const [tk, model, atype, turns, label] of rows) {
      console.log(`    ${fmtComma(tk).padStart(12)}  ${pyStr(model).padEnd(18)} ${pyStr(atype).padEnd(24)} ` +
        `turns=${String(turns).padEnd(4)} ${pyStr(label).slice(0, 52)}`);
    }
    if (total) {
      const share = [...byModel].sort((a, b) => b[1] - a[1])
        .map(([k, v]) => `${pyStr(k)}=${fmtComma(v)} (${pyFixed(v / total * 100, 0)}%)`).join(", ");
      console.log(`  subagent TOTAL: ${fmtComma(total)}  |  ${share}`);
      console.log(`  session grand total (orchestrator + subagents): ${fmtComma(ot + total)}`);
    }
  }

  if (hasCodexLane(sp)) {
    const [rows, deterministic] = codexCost(orch, codexRoot, basis);
    const join = deterministic ? "exact thread-id join" : "correlation (originator+cwd+time) — heuristic";
    console.log(`  --- codex lane (${rows.length} rollouts, ${basis} tokens; ${join}) ---`);
    let ctot = 0;
    for (const [tk, sid, how, rname] of rows) {
      console.log(`    ${fmtComma(tk).padStart(12)}  [${how}] ${pyStr(sid).slice(0, 36)}  ${rname.slice(0, 44)}`);
      ctot += tk;
    }
    if (rows.length) {
      console.log(`  codex TOTAL: ${fmtComma(ctot)}  (NOT in the Claude transcripts; joined from ~/.codex/sessions)`);
      console.log(`  mean joined rollout: ${fmtComma(Math.floor(ctot / rows.length))} tok — the proxy multiplier: an un-joinable ` +
        "failed workstream ≈ this × (discarded-workstream count read from the transcript).");
      console.log("  ** rank this against the Claude buckets above — a failed/nothing-merged codex run " +
        "belongs in the ranking, never dropped because its cost lived off-transcript. **");
    } else {
      console.log("  ** codex lane present but no rollouts joined (ephemeral runs, pruned, or no match). " +
        "Rank its failure events by impact: discarded-workstream count x mean joined per-workstream " +
        "cost, plus the orchestrator's own reaction tokens. Never drop the event. **");
    }
  }
  console.log(`  basis=${basis} excludes cache_read (Claude) / cached_input (codex) unless 'full'.`);
}

// ---- CLI (argparse-compatible surface: same flags, usage text, and exit code 2 on bad input) ----

function usageError(msg) {
  process.stderr.write(USAGE + `${PROG}: error: ${msg}\n`);
  process.exit(2);
}

function main() {
  let parsed;
  try {
    parsed = parseArgs({
      args: process.argv.slice(2),
      allowPositionals: true,
      options: {
        help: { type: "boolean", short: "h" },
        root: { type: "string" },
        basis: { type: "string" },
        "codex-root": { type: "string" },
      },
    });
  } catch (e) {
    if (e.code === "ERR_PARSE_ARGS_UNKNOWN_OPTION") usageError(`unrecognized arguments: ${e.message.match(/'([^']+)'/)?.[1] ?? ""}`);
    if (e.code === "ERR_PARSE_ARGS_INVALID_OPTION_VALUE") {
      const opt = e.message.match(/'(-[^']+)'/)?.[1];
      usageError(opt ? `argument ${opt}: expected one argument` : e.message);
    }
    usageError(e.message);
  }
  const { values, positionals } = parsed;
  if (values.help) { process.stdout.write(HELP); return; }
  const basis = values.basis ?? "billable";
  if (!BASES.includes(basis)) {
    usageError(`argument --basis: invalid choice: ${pyRepr(basis)} (choose from ${BASES.map(pyRepr).join(", ")})`);
  }
  if (!positionals.length) usageError("the following arguments are required: names");
  const root = values.root ?? DEFAULT_ROOT;
  const codexRoot = values["codex-root"] ?? DEFAULT_CODEX_ROOT;
  for (const n of positionals) report(n, root, basis, codexRoot);
}

main();
