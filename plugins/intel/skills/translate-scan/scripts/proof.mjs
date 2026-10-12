// ABOUTME: Proofreads a page's codex transcription against an independent macOS Vision OCR: every spot where the
// ABOUTME: two read different words is cropped at full resolution, read again blind, and the draft corrected there.
import { execFileSync } from "node:child_process";
import path from "node:path";

const MATH_RE = /\\\(.*?\\\)|\\\[[\s\S]*?\\\]/g;

/** Word tokens of a text with their offsets: Markdown and LaTeX markup, [FIGURE] lines and <<CONT>> count as
 * spaces; `math` marks tokens inside \( \) or \[ \]. */
export function tokens(text) {
  const blank = (m) => " ".repeat(m.length);
  const masked = text.replace(/^\[FIGURE[^\]]*\]$/gm, blank).replace(/<<CONT>>/g, blank)
    .replace(/\\[a-zA-Z]+|[#*_`|\\()[\]{}^$]/g, blank);
  const math = [...text.matchAll(MATH_RE)].map((m) => [m.index, m.index + m[0].length]);
  return [...masked.matchAll(/\S+/g)].map((m) => ({ w: m[0], start: m.index, end: m.index + m[0].length,
    math: math.some(([s, e]) => m.index >= s && m.index < e) }));
}

/** Vision's words, each with the index of its line; a word hyphenated across lines is rejoined. */
function lineWords(lines) {
  const out = [];
  lines.forEach((l, li) => {
    for (const { w } of tokens(l.text)) {
      const prev = out[out.length - 1];
      if (prev?.joinNext && /^[a-z]/.test(w)) { prev.w = prev.w.slice(0, -1) + w; prev.joinNext = false; continue; }
      if (prev) prev.joinNext = false;
      out.push({ w, line: li, joinNext: false });
    }
    const last = out[out.length - 1];
    if (last && /[a-z]-$/.test(last.w)) last.joinNext = true;
  });
  return out;
}

const key = (w) => w.toLowerCase().replace(/[^a-z0-9]/g, "");

/** Word-level LCS alignment of the draft against Vision's lines. Returns the runs where the two readings differ
 * in letters or digits, outside math (Vision cannot read math) and not just a page number: each with the draft
 * span [start, end) to replace, the words around it, and the Vision lines it falls on. */
export function disagreements(md, lines) {
  const a = tokens(md), bw = lineWords(lines);
  const ka = a.map((t) => key(t.w)), kb = bw.map((t) => key(t.w));
  const n = a.length, m = bw.length;
  const dp = new Int32Array((n + 1) * (m + 1));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      dp[i * (m + 1) + j] = ka[i] === kb[j] ? dp[(i + 1) * (m + 1) + j + 1] + 1
        : Math.max(dp[(i + 1) * (m + 1) + j], dp[i * (m + 1) + j + 1]);
  const out = [];
  let i = 0, j = 0, run = null;
  const flush = () => {
    if (!run) return;
    const draft = a.slice(run.i, i), x = ka.slice(run.i, i).join(""), y = kb.slice(run.j, j).join("");
    const before = a.slice(Math.max(0, run.i - 3), run.i), after = a.slice(i, i + 2);
    const start = before.length ? before[before.length - 1].end : 0, end = after.length ? after[0].start : md.length;
    const plain = !/[*\\`\n[\]]/.test(md.slice(start, end)) && ![...before, ...draft, ...after].some((t) => t.math);
    if (x !== y && !/^\d*$/.test(x + y) && draft.length + j - run.j < 16 && before.length && after.length && plain) {
      const near = j > run.j ? bw.slice(run.j, j).map((t) => t.line) : [bw[Math.max(0, run.j - 1)]?.line ?? 0];
      out.push({ before: before.map((t) => t.w).join(" "), after: after.map((t) => t.w).join(" "),
        draft: draft.map((t) => t.w).join(" "), vision: bw.slice(run.j, j).map((t) => t.w).join(" "),
        start, end, lines: [Math.min(...near), Math.max(...near)] });
    }
    run = null;
  };
  while (i < n || j < m) {
    if (i < n && j < m && ka[i] === kb[j]) { flush(); i++; j++; continue; }
    run ??= { i, j };
    if (j >= m || (i < n && dp[(i + 1) * (m + 1) + j] >= dp[i * (m + 1) + j + 1])) i++;
    else j++;
  }
  flush();
  return out;
}

/** Crops each spot's Vision lines, with a line of margin above and below, out of the page image at full
 * resolution. Returns the crop paths, one per spot. */
export function cropSpots(image, lines, spots, dir) {
  const [W, H] = execFileSync("magick", ["identify", "-format", "%w %h", image], { encoding: "utf8" }).split(" ").map(Number);
  return spots.map((s, k) => {
    const ls = lines.slice(s.lines[0], s.lines[1] + 1);
    const x0 = Math.min(...ls.map((l) => l.box[0])), x1 = Math.max(...ls.map((l) => l.box[2]));
    const y0 = Math.min(...ls.map((l) => l.box[1])), y1 = Math.max(...ls.map((l) => l.box[3]));
    const pad = (y1 - y0) / ls.length;
    const px = Math.max(0, (x0 - 0.02) * W), py = Math.max(0, (y0 - pad) * H);
    const pw = Math.min(W - px, (x1 - x0 + 0.04) * W), ph = Math.min(H - py, (y1 - y0 + 2 * pad) * H);
    const out = path.join(dir, `spot-${String(k + 1).padStart(2, "0")}.png`);
    execFileSync("magick", [image, "-crop", `${Math.round(pw)}x${Math.round(ph)}+${Math.round(px)}+${Math.round(py)}`, "+repage", out]);
    return out;
  });
}

/** The blind re-reading request: for each crop, the exact text between two anchors, with no draft reading shown,
 * so the answer cannot anchor on either engine's guess. */
export function proofPrompt(spots) {
  const list = spots.map((s, k) => `${k + 1}. image ${k + 1}: the text between "${s.before}" and "${s.after}"`);
  return `Each attached image is a strip cut from a scanned or handwritten page. For each numbered item, read the
image and write the exact text printed or written between the two given anchors (the anchors themselves
excluded), letter by letter, with its punctuation. Small capitals in uppercase. Keep the source's own typos.
If nothing stands between the anchors, the text is "". The anchors may themselves be slightly misread; locate
them by sense.
Answer with JSON only: {"spots": [{"n": 1, "text": "..."}]}.

${list.join("\n")}`;
}

export const READINGS_SCHEMA = {
  type: "object", additionalProperties: false, required: ["spots"],
  properties: { spots: { type: "array", items: { type: "object", additionalProperties: false, required: ["n", "text"],
    properties: { n: { type: "integer" }, text: { type: "string" } } } } },
};

/** Two-of-three vote per spot: the draft span is replaced only when the blind re-reading agrees with Vision (in
 * letters and digits) and not with the draft; any other answer keeps the draft, since a re-reading that matches
 * neither engine has usually lost its anchors. Spots are applied last first so earlier offsets stay valid.
 * Returns [text, one log line per spot]. */
export function applyReadings(md, spots, answer) {
  const json = answer.slice(answer.indexOf("{"), answer.lastIndexOf("}") + 1);
  const read = new Map((JSON.parse(json).spots || []).map((s) => [s.n, String(s.text ?? "")
    .replace(/([a-z])-\s*\n\s*(?=[a-z])/g, "$1").replace(/\s+/g, " ").trim()]));
  const flat = (t) => tokens(t).map((x) => key(x.w)).join("");
  const log = [];
  spots.map((s, k) => [s, read.get(k + 1)]).reverse().forEach(([s, text]) => {
    const old = md.slice(s.start, s.end).trim();
    const fix = text !== undefined && flat(text) === flat(s.vision) && flat(text) !== flat(old);
    const tag = text === undefined ? "NO ANSWER" : fix ? "fixed" : flat(text) === flat(old) ? "kept" : "kept (no majority)";
    log.unshift(`${tag}: …${s.before} [${old}] ${s.after}… vision [${s.vision}] reread [${text ?? ""}]`);
    if (fix) md = md.slice(0, s.start) + (text ? ` ${text} ` : " ") + md.slice(s.end);
  });
  return [md, log.join("\n") + (log.length ? "\n" : "")];
}
