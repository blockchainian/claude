#!/usr/bin/env node
// ABOUTME: Translates a document's text segments (Markdown blocks or HTML fragments) into Chinese through the
// ABOUTME: translate skill's codex exec call, a batch of consecutive segments per call, many calls in flight; resumable.
//
// Usage: translate_segments.mjs <segments.json> <out.json> --format md|html [--title T] [--glossary g.md]
//                               [--context "course material"] [--words 1500] [--jobs 8] [--model gpt-6-luna]
//                               [--effort low]
// segments.json: [{id, text, keep: [verbatim strings the translation must contain], math: [formulas], dollar}], in
// document order, as md_segments.py and html_segments.py write it.
// out.json: {id: translation}; ids already there that pass the checks are skipped, so a rerun only redoes what failed.
import fs from "node:fs";

import { fileURLToPath } from "node:url";

import { runCodex } from "../../translate/scripts/translate.mjs";
import { loadEnvFile } from "../../fetch-x-mentions/scripts/env.mjs";

export const rules = (context) => `You are a professional technical translator (English -> Simplified Chinese) translating
${context} for Chinese readers. Everything you need is in the message: do not run commands, do not read or write
files.

The message gives a JSON object mapping segment ids to English text, in document order: consecutive parts of one
document. Answer with ONLY a JSON object holding exactly the same ids, each mapped to its Chinese translation. No code
fence, no commentary.

Translate every segment faithfully and completely, as a Chinese textbook would print it. Never summarize, skip,
merge or add content, and never move text between segments.
- Code, identifiers, file names, commands, function and type names, and everything inside backticks or <code> tags
  stay exactly as written, in English.
- TeX math between $...$, $$...$$, \\(...\\) or \\[...\\] stays exactly as written.
- Never add TeX or $...$ markup the source does not have; plain text stays plain.
- Use the standard Chinese term for each technical concept; at a key term's first use you may add the English in
  parentheses, e.g. 测试优先编程（test-first programming）. Follow the glossary exactly when one is given.
- People's names, and product, language, library and company names, stay in English.
- Put one space between Chinese and Latin letters or digits. Use full-width Chinese punctuation in Chinese sentences.
- Keep emphasis markup on the matching Chinese words.
- A segment that is only code, a number, a symbol or a proper name is returned unchanged.
`;

export const FORMAT = {
  md: `Each value is Markdown. Keep its structure exactly: the heading marker (#), list markers and their indentation,
"> " quote markers, table pipes and alignment rows, and line breaks. In a link [text](url) translate the text and keep
the (url) byte-identical. Inline \`code\` stays unchanged.`,
  html: `Each value is an HTML fragment. Keep every tag exactly as written, with its attributes (such as k="3"), and
the same nesting; translate only the text between tags, and move a tag pair to where its words land in the Chinese
sentence. The text inside <code> tags stays unchanged. Keep HTML entities valid.`,
};

function parseArgs(argv) {
  const opt = { format: "md", title: "", glossary: null, context: "technical course material", words: 1500, jobs: 8,
    model: "gpt-6-luna", effort: "low" };
  const pos = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith("--")) opt[a.slice(2)] = argv[++i];
    else pos.push(a);
  }
  [opt.segments, opt.out] = pos;
  opt.words = +opt.words; opt.jobs = +opt.jobs;
  if (!opt.segments || !opt.out || !FORMAT[opt.format]) {
    process.stderr.write("usage: translate_segments.mjs <segments.json> <out.json> --format md|html [--title T] [--glossary g.md] [--context C]\n");
    process.exit(2);
  }
  return opt;
}

const words = (s) => (s.match(/[A-Za-z]+/g) || []).length;

// TeX math as html_segments.py finds it: \begin{env}..\end{env}, $$..$$, $..$, \(..\), \[..\].
// The single-dollar alternative, used only on pages that configure $..$ as inline math.
export const DOLLAR = String.raw`|(?<!\\)\$(?:\\.|[^$\\])+?\$`;
export const MATH = /\\begin\{([a-z]+\*?)\}[\s\S]*?\\end\{\1\}|\$\$[\s\S]+?\$\$|(?<!\\)\$(?:\\.|[^$\\])+?\$|\\\([\s\S]+?\\\)|\\\[[\s\S]+?\\\]/g;

/** Returns why a translation is unusable, or null: every keep string must appear (only presence is checked, since
 * Chinese often names a repeated variable once), and on a page with TeX math no formula may hold Chinese. A formula
 * may be dropped or reworded ($i^{th}$ becomes 第 $i$ 个); one that runs into Chinese text means a lost delimiter,
 * which would print broken TeX. */
export function problemWith(seg, zh) {
  if (typeof zh !== "string" || !zh.trim()) return "missing";
  for (const k of new Set(seg.keep || [])) {
    if (!zh.includes(k)) return `lost ${JSON.stringify(k).slice(0, 80)}`;
  }
  const cjk = /[　-〿一-鿿＀-￯]/;
  if (seg.math?.length) {
    const re = new RegExp(seg.dollar ? MATH.source : MATH.source.replace(DOLLAR, ""), "g");
    for (const m of zh.match(re) || []) {
      if (cjk.test(m)) return `broken math ${JSON.stringify(m).slice(0, 80)}`;
    }
  } else {
    // Where the source has no TeX, none may be added ($3f+1$ for 3f+1): nothing would typeset it. A dollar pair
    // around Chinese text is two amounts of money, not a formula.
    for (const m of zh.match(MATH) || []) {
      if (!cjk.test(m) && !seg.text.includes(m)) return `added math ${JSON.stringify(m).slice(0, 80)}`;
    }
  }
  return null;
}

/** Consecutive segments grouped into batches of about `budget` English words. */
export function batches(segs, budget) {
  const out = [];
  let cur = [], n = 0;
  for (const s of segs) {
    if (cur.length && n + words(s.text) > budget) { out.push(cur); cur = []; n = 0; }
    cur.push(s); n += words(s.text);
  }
  if (cur.length) out.push(cur);
  return out;
}

export function parseAnswer(text) {
  const body = text.trim().replace(/^```(?:json)?\s*\n/, "").replace(/\n```\s*$/, "");
  return JSON.parse(body.slice(body.indexOf("{"), body.lastIndexOf("}") + 1));
}

async function translateBatch(batch, opt, glossary) {
  const head = [`Document: ${opt.title}`];
  if (glossary) head.push("Glossary (use these renderings):", glossary.trim());
  const src = Object.fromEntries(batch.map((s) => [s.id, s.text]));
  const prompt = head.join("\n") + "\n\nSegments:\n\n" + JSON.stringify(src, null, 1) + "\n";
  const [answer] = await runCodex(prompt, opt.model, opt.effort, "priority", { rules: rules(opt.context) + "\n" + FORMAT[opt.format] });
  return parseAnswer(answer);
}

async function main() {
  loadEnvFile();
  const opt = parseArgs(process.argv.slice(2));
  const glossary = opt.glossary ? fs.readFileSync(opt.glossary, "utf8") : null;
  const segs = JSON.parse(fs.readFileSync(opt.segments, "utf8")).filter((s) => words(s.text) > 0);
  const done = fs.existsSync(opt.out) ? JSON.parse(fs.readFileSync(opt.out, "utf8")) : {};
  // The last rejected answer per segment is kept beside out.json, to read why it failed or accept it by hand.
  const rejected = {};
  const save = () => {
    fs.writeFileSync(opt.out, JSON.stringify(done, null, 1));
    fs.writeFileSync(opt.out.replace(/\.json$/, "") + ".rejected.json", JSON.stringify(rejected, null, 1));
  };
  // Saved translations are rechecked, so a stricter keep list redoes only the segments it now rejects. Ids listed in
  // <out>.accepted.json were read and accepted by hand after failing the checks (a model that writes `delete()` for
  // the source's `delete` every time), so they are kept as they are.
  const acceptedFile = opt.out.replace(/\.json$/, "") + ".accepted.json";
  const accepted = new Set(fs.existsSync(acceptedFile) ? JSON.parse(fs.readFileSync(acceptedFile, "utf8")) : []);
  let todo = segs.filter((s) => !(accepted.has(s.id) && Object.hasOwn(done, s.id)) && problemWith(s, done[s.id]));
  console.log(`${todo.length} of ${segs.length} segments to translate`);
  save();  // a document with nothing to translate (a page of pseudocode) still gets its empty out.json
  // A batch's failed segments get a second pass in smaller batches and a third one alone at medium effort; what fails
  // all three stays English and is reported.
  for (const [budget, effort] of [[opt.words, opt.effort], [Math.max(150, Math.round(opt.words / 6)), opt.effort], [1, "medium"]]) {
    const queue = batches(todo, budget);
    const failed = [];
    let next = 0;
    const worker = async () => {
      while (next < queue.length) {
        const batch = queue[next++];
        let answer = {};
        try {
          answer = await translateBatch(batch, { ...opt, effort }, glossary);
        } catch (e) {
          console.log(`batch ${batch[0].id}..: ${e.message.slice(0, 200)}`);
        }
        for (const s of batch) {
          const why = problemWith(s, answer[s.id]);
          if (why) { failed.push(s); rejected[s.id] = answer[s.id]; console.log(`retry ${s.id}: ${why}`); } else done[s.id] = answer[s.id];
        }
        save();
      }
    };
    await Promise.all(Array.from({ length: Math.min(opt.jobs, queue.length) }, worker));
    todo = failed;
    if (!todo.length) break;
  }
  for (const s of todo) console.log(`FAILED ${s.id}`);
  console.log(`done: ${segs.length - todo.length}/${segs.length} segments`);
  process.exit(todo.length ? 1 : 0);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  await main();
}
