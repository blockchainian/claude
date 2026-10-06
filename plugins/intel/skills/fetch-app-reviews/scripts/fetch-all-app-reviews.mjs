// ABOUTME: Fetches App Store reviews for every app in <INTEL_STATE_DIR>/reviews/apps.json,
// ABOUTME: bottom rank to top, retrying any app left incomplete until all finish.
//
// Usage:
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-app-reviews/scripts/fetch-all-app-reviews.mjs
//
// Reads <INTEL_STATE_DIR>/reviews/apps.json, orders apps by rank descending (25 -> 1), and runs the
// review fetch for each into <INTEL_STATE_DIR>/reviews/<name>.json through one residential-proxy
// dispatcher. An app whose fetch hit a network/proxy failure comes back incomplete and is
// retried in the next pass, so a transient error never leaves an app half-scraped.

import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { fetchAppReviews } from "./fetch-app-reviews.mjs";
import { loadEnvFile, stateDir } from "../../fetch-x-mentions/scripts/env.mjs";

loadEnvFile();

const MAX_PASSES = 6;
const APPS = join(stateDir(), "reviews", "apps.json");

const apps = JSON.parse(await readFile(APPS, "utf8"));
apps.sort((a, b) => b.rank - a.rank); // bottom rank first (25 -> 1)

const done = new Map(); // appId -> count

for (let pass = 1; pass <= MAX_PASSES; pass++) {
  const pending = apps.filter((a) => !done.has(a.appId));
  if (pending.length === 0) break;
  process.stderr.write(
    `\n##### PASS ${pass} — ${pending.length} app(s) #####\n`,
  );
  for (const app of pending) {
    const label = `#${app.rank} ${app.name} (${app.appId})`;
    process.stderr.write(`\n=== ${label} ===\n`);
    try {
      const res = await fetchAppReviews(app.appId, app.name, {
        onCountry: ({ cc, total, ok }) =>
          process.stderr.write(
            `  ${cc}: total=${total}${ok ? "" : " FAILED"}\n`,
          ),
      });
      if (res.complete) {
        done.set(app.appId, res.count);
        process.stderr.write(`DONE ${label}: ${res.count} reviews\n`);
      } else {
        process.stderr.write(
          `INCOMPLETE ${label}: ${res.count} so far, will retry\n`,
        );
      }
    } catch (err) {
      process.stderr.write(`ERROR ${label}: ${err?.message ?? err}\n`);
    }
  }
}

process.stderr.write("\n=== ALLDONE ===\n");
let total = 0;
for (const app of apps) {
  const count = done.get(app.appId);
  total += count ?? 0;
  console.log(
    count === undefined
      ? `#${app.rank} ${app.name}: INCOMPLETE (still failing after ${MAX_PASSES} passes)`
      : `#${app.rank} ${app.name}: ${count} reviews`,
  );
}
console.log(`TOTAL: ${total} reviews across ${done.size}/${apps.length} apps`);
