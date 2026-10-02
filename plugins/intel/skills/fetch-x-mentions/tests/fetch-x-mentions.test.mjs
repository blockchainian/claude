// ABOUTME: Tests the pure parts of fetch-x-mentions.mjs: the per-day search window, the
// ABOUTME: progress-file contract, gap detection, run planning, day merging, retries and sorting.

import assert from "node:assert/strict";
import { test } from "node:test";
import { DatabaseSync } from "node:sqlite";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  dayQuery,
  drainPlan,
  duration,
  fetchDay,
  getPage,
  isGap,
  loadAccounts,
  mergeDay,
  openProgress,
  parseArgs,
  planDays,
  sortTweets,
  untilInstant,
} from "../scripts/fetch-x-mentions.mjs";

test("day window uses explicit UTC instants, not bare dates", () => {
  assert.equal(
    dayQuery('"demo fun"', "2026-03-10"),
    '"demo fun" since:2026-03-10_00:00:00_UTC until:2026-03-11_00:00:00_UTC',
  );
  assert.equal(
    dayQuery('"demo fun"', "2026-03-10", "2026-03-10_13:45:02_UTC"),
    '"demo fun" since:2026-03-10_00:00:00_UTC until:2026-03-10_13:45:02_UTC',
  );
});

test("a fill window ends one second after the oldest tweet already held", () => {
  assert.equal(
    untilInstant("2026-03-10T13:45:01.000Z"),
    "2026-03-10_13:45:02_UTC",
  );
});

test("--daily-limit and --refill [n] are parsed, the cap rounded up to whole pages", () => {
  assert.deepEqual(parseArgs(["a", "b", "--daily-limit", "30", "--refill"]), {
    positional: ["a", "b"],
    dailyLimit: 30,
    maxPages: 2,
    refill: 0,
  });
  assert.equal(parseArgs(["a", "--refill", "2"]).refill, 2);
  assert.equal(parseArgs(["a", "--refill=1"]).refill, 1);
  assert.deepEqual(parseArgs(["a", "--refill", "b"]).positional, ["a", "b"]);
  assert.equal(parseArgs(["a"]).dailyLimit, 1000);
  assert.equal(parseArgs(["a"]).refill, null);
  assert.throws(() => parseArgs(["a", "--fill"]), /Unknown option/);
  assert.throws(
    () => parseArgs(["a", "--daily-limit", "0"]),
    /positive integer/,
  );
});

test("a fresh progress file records the query and limit with no days", () => {
  assert.deepEqual(openProgress(null, "q", 1000), {
    query: "q",
    dailyLimit: 1000,
    days: {},
  });
});

test("an existing progress file is reused only for the same query and limit", () => {
  const saved = {
    query: "q",
    dailyLimit: 1000,
    days: { "2026-09-01": { count: 998 } },
  };
  assert.deepEqual(openProgress(saved, "q", 1000), saved);
  assert.throws(() => openProgress(saved, "other", 1000), /query/);
  assert.throws(() => openProgress(saved, "q", 500), /dailyLimit/);
  assert.throws(() => openProgress(["2026-09-01"], "q", 1000), /days/);
});

test("a gap is an empty day or one cut at a page boundary below the limit", () => {
  assert.equal(isGap({ count: 0 }, 1000), true);
  assert.equal(isGap({ count: 40 }, 1000), true);
  assert.equal(isGap({ count: 500 }, 1000), true);
  assert.equal(isGap({ count: 41 }, 1000), false);
  assert.equal(isGap({ count: 1000 }, 1000), false);
  assert.equal(isGap({ count: 998 }, 1000), false);
});

test("planning fetches missing days, and with --refill n the gaps refilled at most n times", () => {
  const progress = {
    dailyLimit: 1000,
    days: {
      "2026-03-01": { count: 998, oldest: "2026-03-01T00:00:09.000Z" },
      "2026-03-02": { count: 0 },
      "2026-03-03": {
        count: 40,
        oldest: "2026-03-03T20:00:00.000Z",
        refill: 1,
      },
      "2026-03-05": { count: 0, refill: 2 },
    },
  };
  assert.deepEqual(planDays(progress, "2026-03-01", "2026-03-06", null), [
    { day: "2026-03-04", until: null },
  ]);
  assert.deepEqual(planDays(progress, "2026-03-01", "2026-03-06", 0), [
    { day: "2026-03-04", until: null },
    { day: "2026-03-02", until: null },
  ]);
  assert.deepEqual(planDays(progress, "2026-03-01", "2026-03-06", 1), [
    { day: "2026-03-04", until: null },
    { day: "2026-03-03", until: "2026-03-03_20:00:01_UTC" },
    { day: "2026-03-02", until: null },
  ]);
  assert.equal(planDays(progress, "2026-03-01", "2026-03-06", 2).length, 4);
});

const tw = (id, iso) => ({ id, created_at: new Date(iso).toUTCString() });

test("a first fetch records count and oldest, and no refill field", () => {
  const got = [
    tw("2", "2026-03-02T10:00:00Z"),
    tw("1", "2026-03-02T09:00:00Z"),
  ];
  assert.deepEqual(mergeDay(undefined, got, false), {
    count: 2,
    oldest: "2026-03-02T09:00:00.000Z",
  });
  assert.deepEqual(mergeDay(undefined, [], false), { count: 0 });
});

test("a refill adds only the tweets older than what the day held and counts the refill", () => {
  const held = { count: 40, oldest: "2026-03-03T20:00:00.000Z" };
  const got = [
    tw("b", "2026-03-03T20:00:00Z"), // the boundary tweet comes back and is not new
    tw("c", "2026-03-03T19:00:00Z"),
    tw("d", "2026-03-03T08:00:00Z"),
  ];
  assert.deepEqual(mergeDay(held, got, true), {
    count: 42,
    oldest: "2026-03-03T08:00:00.000Z",
    refill: 1,
  });
  assert.deepEqual(mergeDay({ ...held, refill: 1 }, [], true), {
    ...held,
    refill: 2,
  });
  assert.deepEqual(mergeDay({ count: 0 }, [], true), { count: 0, refill: 1 });
  assert.deepEqual(mergeDay({ count: 0, refill: 1 }, got, true), {
    count: 3,
    oldest: "2026-03-03T08:00:00.000Z",
    refill: 2,
  });
});

const page = (ids, cursor) => ({
  status: 200,
  body: {
    data: {
      search_by_raw_query: {
        search_timeline: {
          timeline: {
            instructions: [
              {
                entries: [
                  ...ids.map((id) => ({
                    entryId: `tweet-${id}`,
                    content: {
                      itemContent: {
                        tweet_results: {
                          result: { rest_id: id, legacy: { full_text: "x" } },
                        },
                      },
                    },
                  })),
                  { entryId: "cursor-bottom-1", content: { value: cursor } },
                ],
              },
            ],
          },
        },
      },
    },
  },
});

test("an empty page ends the day without a retry", async () => {
  const responses = [page(["1", "2"], "c1"), page([], "c2"), page(["3"], "c3")];
  const calls = [];
  const fetchPage = async (url) => {
    calls.push(url);
    return responses.shift();
  };
  const waits = [];
  const got = await fetchDay(
    {},
    "q",
    "2026-01-01",
    null,
    [],
    50,
    fetchPage,
    async (ms) => {
      waits.push(ms);
    },
  );
  assert.deepEqual(
    got.map((t) => t.id),
    ["1", "2"],
  );
  assert.equal(calls.length, 2);
  assert.deepEqual(waits, [800]);
});

test("sorting dedups by id and orders newest first", () => {
  const lines = [
    tw("a", "2026-03-01T00:00:00Z"),
    tw("b", "2026-03-03T00:00:00Z"),
    { ...tw("a", "2026-03-01T00:00:00Z"), likes: 5 },
    tw("c", "2026-03-02T00:00:00Z"),
  ];
  assert.deepEqual(
    sortTweets(lines).map((t) => [t.id, t.likes]),
    [
      ["b", undefined],
      ["c", undefined],
      ["a", 5],
    ],
  );
});

test("accounts are read from the store and labelled by username", () => {
  const dbPath = join(mkdtempSync(join(tmpdir(), "fxm-")), "s.sqlite");
  const db = new DatabaseSync(dbPath);
  // The secrets-manager schema: a `status` enum, no `active`/`id` column.
  db.exec(
    "CREATE TABLE x (email TEXT PRIMARY KEY, username TEXT, auth_token TEXT, ct0 TEXT, status TEXT)",
  );
  db.prepare(
    "INSERT INTO x (email, username, auth_token, ct0, status) VALUES " +
      "(?, ?, ?, ?, 'active'), (?, ?, ?, ?, 'active'), (?, ?, ?, ?, 'restricted')",
  ).run("a@e", "alice", "a1", "c1", "b@e", "bob", "a2", "c2", "c@e", "carol", "a3", "c3");
  db.close();
  process.env.SECRETS_DB = dbPath;
  process.env.RESIDENTIAL_PROXY_URL = "http://u:p@proxy.test:8080";
  // Only the two active accounts are read, ordered by username; the restricted one is skipped.
  assert.deepEqual(
    loadAccounts().map((a) => a.label),
    ["alice", "bob"],
  );
});

test("durations read as minutes and seconds", () => {
  assert.equal(duration(804000), "13m 24s");
  assert.equal(duration(601000), "10m 1s");
  assert.equal(duration(45000), "45s");
  assert.equal(duration(120000), "2m 0s");
});

const res = (status, body = "", headers = {}) => ({
  status,
  headers: { get: (k) => headers[k] ?? null },
  json: async () => JSON.parse(body),
  text: async () => body,
});

test("a page retries proxy errors, bad bodies and 5xx with doubling waits, then gives up", async () => {
  const outcomes = [
    () => {
      throw Object.assign(new Error("socket hang up"), { code: "ECONNRESET" });
    },
    () => res(200, ""),
    () => res(502, "bad gateway"),
    () => res(503),
    () => res(500),
    () => res(200, "{"),
  ];
  const waits = [];
  const log = [];
  await assert.rejects(
    getPage(
      "u",
      { label: "acct1" },
      async () => outcomes.shift()(),
      async (ms) => {
        waits.push(ms);
      },
      (line) => log.push(line),
    ),
    /failed after 6 attempts/,
  );
  assert.deepEqual(waits, [1000, 2000, 4000, 8000, 16000, 32000]);
  assert.equal(log.length, 6);
  assert.equal(log[0], "  [acct1] ECONNRESET");
  assert.equal(log[2], "  [acct1] 502");
});

test("a 403 with an HTML body is an edge/proxy block — retried, not fatal", async () => {
  const outcomes = [
    () => res(403, "<!DOCTYPE html><html>Sorry, you have been blocked</html>"),
    () => res(200, JSON.stringify({ ok: true })),
  ];
  const log = [];
  const got = await getPage(
    "u",
    { label: "acct1" },
    async () => outcomes.shift()(),
    async () => {},
    (line) => log.push(line),
  );
  assert.deepEqual(got, { status: 200, body: { ok: true } });
  assert.match(log[0], /403 edge block/);
});

test("a 4xx with a JSON body is a broken request — raised at once", async () => {
  await assert.rejects(
    getPage(
      "u",
      { label: "acct1" },
      async () => res(400, JSON.stringify({ errors: [{ message: "features cannot be null" }] })),
      async () => {},
      () => {},
    ),
    /SearchTimeline 400/,
  );
});

test("a 429 waits for X's reset without spending an attempt", async () => {
  const reset = Math.ceil(Date.now() / 1000) + 30;
  const outcomes = [
    () => res(429, "", { "x-rate-limit-reset": String(reset) }),
    () => res(200, JSON.stringify({ ok: true })),
  ];
  const waits = [];
  const got = await getPage(
    "u",
    { label: "acct1" },
    async () => outcomes.shift()(),
    async (ms) => {
      waits.push(ms);
    },
    () => {},
  );
  assert.deepEqual(got, { status: 200, body: { ok: true } });
  assert.equal(waits.length, 1);
  // Waits exactly until the reset instant (~30s, reset is ceil'd to the second), no padding.
  assert.ok(waits[0] >= 30000 && waits[0] < 31000);
});

test("a 429 is never given up on — it waits the reset (0 for a past one) and continues", async () => {
  const past = Math.floor(Date.now() / 1000) - 60;
  const outcomes = [
    () => res(429, "", { "x-rate-limit-reset": String(past) }),
    () => res(429, "", { "x-rate-limit-reset": String(past) }),
    () => res(200, JSON.stringify({ ok: true })),
  ];
  const waits = [];
  const got = await getPage(
    "u",
    { label: "acct1" },
    async () => outcomes.shift()(),
    async (ms) => {
      waits.push(ms);
    },
    () => {},
  );
  assert.deepEqual(got, { status: 200, body: { ok: true } });
  // A reset already past means the window reopened, so retry at once (0 wait), no floor, no give-up.
  assert.deepEqual(waits, [0, 0]);
});

test("a lane that fails is paused, its day goes back to the queue, and the others drain it", async () => {
  const plan = [{ day: "d1" }, { day: "d2" }, { day: "d3" }, { day: "d4" }];
  const done = [];
  const fetchEntry = async (acct, entry) => {
    if (acct.label === "acct1")
      throw new Error("SearchTimeline failed after 6 attempts");
    done.push(`${acct.label}:${entry.day}`);
  };
  const log = [];
  const left = await drainPlan(
    [{ label: "acct0" }, { label: "acct1" }],
    plan,
    fetchEntry,
    async () => {},
    (line) => log.push(line),
  );
  assert.deepEqual(left, { paused: ["acct1"], left: 0 });
  assert.deepEqual(done.sort(), [
    "acct0:d1",
    "acct0:d2",
    "acct0:d3",
    "acct0:d4",
  ]);
  assert.match(log.join("\n"), /acct1.*paused/);
});

test("when every account is paused the remaining days are reported, not fetched", async () => {
  const plan = [{ day: "d1" }, { day: "d2" }, { day: "d3" }];
  const left = await drainPlan(
    [{ label: "acct0" }],
    plan,
    async () => {
      throw new Error("SearchTimeline 403: locked");
    },
    async () => {},
    () => {},
  );
  assert.deepEqual(left, { paused: ["acct0"], left: 3 });
});
