#!/usr/bin/env node
// ABOUTME: Translates the unique text blocks of a slide deck (blocks.json from slides.py extract) into Chinese
// through the translate-epub skill's Codex Responses client, in batches run in parallel, and writes zh.json.
// Usage: translate_slides.mjs <work dir> --title "<course: deck>" [--glossary g.md] [--model M] [--effort E]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runCodex } from "../../epub/scripts/translate.mjs";

const RULES = `You translate the text of English lecture slides into Simplified Chinese.

The input is a JSON object mapping ids to slide text blocks. Reply with only a JSON object mapping the same ids to
their Chinese translations: no prose, no code fence.

- Write natural, concise Chinese suited to a slide: keep a heading a heading and a fragment a fragment.
- Keep every inline tag exactly: <s1>…</s1>, <c1>…</c1> and so on mark text in another style. Put the matching
  Chinese inside the same tag, keeping each tag's count and order. Text inside <cN> tags is code: copy it verbatim.
- Copy every ⟦En⟧ token verbatim, in its place in the sentence: it stands for a picture of code, math or an emoji.
- <br> separates list items: keep each one, translating item by item.
- Keep verbatim: code, identifiers, function and variable names, file names, URLs, email addresses, people's
  names, course numbers, product and system names (Therac-25, Cobalt-60, Piazza, Canvas).
- Keep the source's &amp; &lt; &gt; escapes as they are.
- A block with nothing to translate (a name, a URL, a number) maps to its text unchanged.
- Translate technical terms the way Chinese computer science textbooks do; give an uncommon term once as
  中文（English）. Use full-width Chinese punctuation in Chinese text.`;

export function parseArgs(argv) {
  const opt = { work: null, title: "", glossary: null, model: "gpt-6-luna", effort: "low", serviceTier: "priority",
    batch: 80, force: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--title") opt.title = argv[++i];
    else if (a === "--glossary") opt.glossary = argv[++i];
    else if (a === "--model") opt.model = argv[++i];
    else if (a === "--effort") opt.effort = argv[++i];
    else if (a === "--batch") opt.batch = Number(argv[++i]);
    else if (a === "--force") opt.force = true;
    else opt.work = a;
  }
  if (!opt.work) throw new Error("usage: translate_slides.mjs <work dir> --title T [--glossary g.md]");
  return opt;
}

// Inline tags must keep their order; picture tokens may move, since Chinese word order differs.
const tagsOf = (s) => (s.match(/<\/?[sc]\d+>|<br>/g) || []).join(" ") + " | " + (s.match(/⟦E\d+⟧/g) || []).sort().join(" ");
const codeOf = (s) => (s.match(/<c\d+>.*?<\/c\d+>/g) || []).join("\u0000");

/** Why a translation is unusable, or null: inline tags, picture tokens and code runs must survive unchanged. */
export function problem(src, zh) {
  if (typeof zh !== "string" || !zh.trim()) return "empty";
  if (tagsOf(src) !== tagsOf(zh)) return `tags differ: ${tagsOf(src)} vs ${tagsOf(zh)}`;
  if (codeOf(src) !== codeOf(zh)) return "code runs changed";
  return null;
}

async function translateBatch(items, opt, glossary) {
  const input = Object.fromEntries(items.map((t, i) => [`t${i + 1}`, t]));
  const head = [`Slides: ${opt.title}`];
  if (glossary) head.push("Glossary (use these renderings):", glossary.trim());
  const prompt = head.join("\n") + "\n\nInput:\n" + JSON.stringify(input, null, 1) + "\n";
  const [answer] = await runCodex(prompt, opt.model, opt.effort, opt.serviceTier, { rules: RULES });
  const json = JSON.parse(answer.slice(answer.indexOf("{"), answer.lastIndexOf("}") + 1));
  return items.map((_, i) => json[`t${i + 1}`]);
}

async function main() {
  const opt = parseArgs(process.argv.slice(2));
  const blocks = JSON.parse(fs.readFileSync(path.join(opt.work, "blocks.json"), "utf8"));
  const outFile = path.join(opt.work, "zh.json");
  const done = !opt.force && fs.existsSync(outFile) ? JSON.parse(fs.readFileSync(outFile, "utf8")) : {};
  const glossary = opt.glossary ? fs.readFileSync(opt.glossary, "utf8") : null;
  const todo = [...new Set(blocks.map((b) => b.text))].filter((t) => !done[t]);
  const batches = [];
  for (let i = 0; i < todo.length; i += opt.batch) batches.push(todo.slice(i, i + opt.batch));
  console.log(`${todo.length} texts to translate in ${batches.length} batches with ${opt.model}/${opt.effort}`);
  let failed = [];
  await Promise.all(batches.map(async (items, n) => {
    for (let attempt = 1; attempt <= 2 && items.length; attempt++) {
      let zh;
      try {
        zh = await translateBatch(items, opt, glossary);
      } catch (e) {
        console.log(`batch ${n + 1} attempt ${attempt}: ${e.message.slice(0, 300)}`);
        continue;
      }
      const retry = [];
      items.forEach((t, i) => {
        const why = problem(t, zh[i]);
        if (why) { retry.push(t); if (attempt === 2) console.log(`REJECTED ${JSON.stringify(t).slice(0, 80)}: ${why}`); }
        else done[t] = zh[i];
      });
      items = retry;
    }
    if (items.length) {
      // Last resort for texts whose style tags keep coming back wrong: translate them without the <sN> runs, so the
      // block is set in its base style but in Chinese. Code, pictures and list breaks must still survive.
      const plain = items.map((t) => t.replace(/<\/?s\d+>/g, ""));
      try {
        const zh = await translateBatch(plain, opt, glossary);
        const left = [];
        items.forEach((t, i) => (problem(plain[i], zh[i]) ? left.push(t) : (done[t] = zh[i])));
        items = left;
      } catch (e) {
        console.log(`batch ${n + 1} plain retry: ${e.message.slice(0, 300)}`);
      }
    }
    // What is still left gets one call each at medium effort, with its tags and then without: a batch of other
    // texts is what usually makes the model change a code run or a picture token in this one.
    for (const t of items.splice(0)) {
      let ok = false;
      for (const text of [t, t.replace(/<\/?s\d+>/g, "")]) {
        try {
          const [zh] = await translateBatch([text], { ...opt, effort: "medium" }, glossary);
          if (!problem(text, zh)) { done[t] = zh; ok = true; break; }
        } catch (e) {
          console.log(`single retry: ${e.message.slice(0, 300)}`);
        }
      }
      if (!ok) items.push(t);
    }
    failed = failed.concat(items);
    fs.writeFileSync(outFile, JSON.stringify(done, null, 1));
  }));
  fs.writeFileSync(outFile, JSON.stringify(done, null, 1));
  console.log(`done: ${Object.keys(done).length} translated, ${failed.length} failed`);
  process.exit(failed.length ? 1 : 0);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  main().catch((e) => { console.error(e); process.exit(1); });
}
