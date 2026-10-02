#!/usr/bin/env node
// ABOUTME: Deterministic grounding for an App Store reviews JSON — rating distribution, averages, geography, dates.
// ABOUTME: Also splits every review's text into neg/mid/pos files so the agent can read all of them, not sample.
//
// Ground an App Store reviews dataset with reproducible facts.
//
// Input is a reviews JSON with a top-level `reviews` array whose items carry
// `rating` (1-5), `title`, `body`, `date`, `country` and optionally
// `developerResponseBody`. Prints a stats JSON to stdout. With --dump-dir, also
// writes neg.txt (1-2 star), mid.txt (3 star) and pos.txt (4-5 star), one review
// per line as `[rating|country] title :: body`, so the agent reads the full text.
//
// Nothing here interprets — it only counts. Themes and quotes are the agent's job.
//
// Usage: stats.mjs <reviews.json> [--dump-dir <dir>]
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const USAGE = "usage: stats.mjs [-h] [--dump-dir DUMP_DIR] json";

// Python's round(): correctly rounded on the exact decimal value, ties to even.
// Math.round would turn an avg of 3.125 into 3.13 where Python prints 3.12.
export function pyRound(x, digits = 0) {
  const fixed = x.toFixed(digits);
  const exact = x.toFixed(30);
  const tail = exact.slice(exact.indexOf(".") + 1 + digits);
  if (!/^50*$/.test(tail)) return Number(fixed);
  // Exact tie: keep the truncated value when its last digit is even, else step away from zero.
  const truncated = exact.slice(0, exact.indexOf(".") + 1 + digits).replace(/\.$/, "");
  const lastDigit = Number(truncated.at(-1));
  const step = (x < 0 ? -1 : 1) / 10 ** digits;
  return lastDigit % 2 === 0 ? Number(truncated) : Number((Number(truncated) + step).toFixed(digits));
}

export function load(path) {
  const data = JSON.parse(readFileSync(path, "utf-8"));
  const reviews = Array.isArray(data) ? data : (data && typeof data === "object" ? data.reviews : null) ?? [];
  if (!Array.isArray(reviews) || reviews.length === 0) {
    console.error(`no reviews found in ${path}`);
    process.exit(1);
  }
  return reviews;
}

function countBy(values) {
  const counts = new Map();
  for (const v of values) counts.set(v, (counts.get(v) ?? 0) + 1);
  return counts;
}

// Python's Counter.most_common: by count descending, ties in first-seen order.
function mostCommon(counts, n) {
  return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, n);
}

function toInt(value) {
  const n = Number(value || 0);
  if (!Number.isFinite(n)) throw new Error(`invalid rating: ${JSON.stringify(value)}`);
  return Math.trunc(n);
}

export function computeStats(reviews) {
  for (const x of reviews) x.rating = toInt(x.rating);
  const ratings = countBy(reviews.map((x) => x.rating));
  const total = reviews.length;
  const avg = total ? reviews.reduce((sum, x) => sum + x.rating, 0) / total : 0;
  const countries = countBy(reviews.map((x) => x.country ?? null));
  const us = countries.get("us") ?? 0;
  const dates = reviews
    .filter((x) => x.date)
    .map((x) => String(x.date))
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const neg = reviews.filter((x) => x.rating <= 2);
  const mid = reviews.filter((x) => x.rating === 3);
  const pos = reviews.filter((x) => x.rating >= 4);

  const out = {
    total,
    ratings: Object.fromEntries([1, 2, 3, 4, 5].map((k) => [String(k), ratings.get(k) ?? 0])),
    avg: pyRound(avg, 2),
    us,
    nonUs: total - us,
    usPct: total ? pyRound((100 * us) / total) : 0,
    countries: countries.size,
    topCountries: mostCommon(countries, 8),
    dateRange: dates.length ? [dates[0].slice(0, 10), dates.at(-1).slice(0, 10)] : null,
    developerResponses: reviews.filter((x) => x.developerResponseBody).length,
    n_dislike_1_3: total - pos.length,
    n_like_4_5: pos.length,
  };
  return { out, neg, mid, pos };
}

// Same text as Python's json.dumps(indent=2): avg is always a float there, so 4 prints as 4.0.
export function formatStats(out) {
  return JSON.stringify(out, null, 2).replace(/^(\s*"avg": -?\d+)(,?)$/m, "$1.0$2");
}

export function formatDump(items) {
  return items
    .map((x) => {
      const title = String(x.title || "").replaceAll("\n", " ");
      const body = String(x.body || "").replaceAll("\n", " ");
      return `[${x.rating}|${x.country ?? "None"}] ${title} :: ${body}\n`;
    })
    .join("");
}

function dump(path, items) {
  writeFileSync(path, formatDump(items), "utf-8");
}

function parseCli(argv) {
  let parsed;
  try {
    parsed = parseArgs({
      args: argv,
      options: { "dump-dir": { type: "string" }, help: { type: "boolean", short: "h" } },
      allowPositionals: true,
      strict: true,
    });
  } catch (err) {
    console.error(`${USAGE}\nstats.mjs: error: ${err.message}`);
    process.exit(2);
  }
  if (parsed.values.help) {
    console.log(`${USAGE}\n\npositional arguments:\n  json                 path to the reviews JSON\n\noptions:\n  -h, --help           show this help message and exit\n  --dump-dir DUMP_DIR  write neg/mid/pos text dumps here`);
    process.exit(0);
  }
  if (parsed.positionals.length !== 1) {
    const problem = parsed.positionals.length ? `unrecognized arguments: ${parsed.positionals.slice(1).join(" ")}` : "the following arguments are required: json";
    console.error(`${USAGE}\nstats.mjs: error: ${problem}`);
    process.exit(2);
  }
  return { json: parsed.positionals[0], dumpDir: parsed.values["dump-dir"] };
}

export function main(argv) {
  const args = parseCli(argv);
  const reviews = load(args.json);
  const { out, neg, mid, pos } = computeStats(reviews);
  console.log(formatStats(out));

  if (args.dumpDir) {
    mkdirSync(args.dumpDir, { recursive: true });
    dump(join(args.dumpDir, "neg.txt"), [...neg].sort((a, b) => a.rating - b.rating));
    dump(join(args.dumpDir, "mid.txt"), mid);
    dump(join(args.dumpDir, "pos.txt"), pos);
    console.error(`\nwrote neg(${neg.length}) mid(${mid.length}) pos(${pos.length}) to ${args.dumpDir}`);
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2));
