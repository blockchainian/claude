// ABOUTME: Tests the pure parts of fetch-x-user-posts.mjs: handle normalization, the year window, the
// ABOUTME: per-stream search query, arg and file parsing, progress reuse, work planning, and paging.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  attemptUntil,
  classifyUser,
  collectHandles,
  extractTweet,
  fetchStream,
  handlesFromFile,
  isSettled,
  normHandle,
  oneYearBefore,
  openProgress,
  parseArgs,
  parseUserTimeline,
  planResolve,
  planWork,
  streamComplete,
  streamQuery,
  STREAMS,
  userTimelineUrl,
  userUrl,
} from "../scripts/fetch-x-user-posts.mjs";

// Offline URL fixtures do not depend on the user’s request configuration.
process.env.X_SEARCH_QUERY_ID = "fixture-search-query";
process.env.X_USER_QUERY_ID = "fixture-user-query";
process.env.X_TIMELINE_QUERY_ID = "fixture-timeline-query";

test("a handle is stripped of @, url and path, lowercased, and validated", () => {
  assert.equal(normHandle("@Vali_ETH"), "vali_eth");
  assert.equal(normHandle("https://x.com/frankdegods"), "frankdegods");
  assert.equal(normHandle("https://twitter.com/foo/status/1"), "foo");
  assert.equal(normHandle("  bar123  "), "bar123");
  assert.equal(normHandle(""), null);
  assert.equal(normHandle(null), null);
  assert.equal(normHandle("way_too_long_a_handle"), null); // > 15 chars
  assert.equal(normHandle("has spaces"), null);
});

test("the default window is the year before until", () => {
  assert.equal(oneYearBefore("2026-09-24"), "2025-09-24");
  assert.equal(oneYearBefore("2024-02-29"), "2023-03-01"); // Feb 29 → no such day in 2023, rolls over
});

test("a stream query pins from:user and the filter to a UTC window", () => {
  assert.equal(
    streamQuery("vali_eth", "-filter:replies -filter:nativeretweets", "2025-09-24", "2026-09-24_00:00:00_UTC"),
    "from:vali_eth -filter:replies -filter:nativeretweets since:2025-09-24_00:00:00_UTC until:2026-09-24_00:00:00_UTC",
  );
  assert.deepEqual(STREAMS.map((s) => s.name), ["tweets", "replies"]);
});

test("an attempt's until is the window end first, then one second past the oldest held", () => {
  assert.equal(attemptUntil(null, "2026-09-24"), "2026-09-24_00:00:00_UTC");
  assert.equal(attemptUntil("2026-03-10T13:45:01.000Z", "2026-09-24"), "2026-03-10_13:45:02_UTC");
});

test("args split into users, files, window and page cap", () => {
  assert.deepEqual(
    parseArgs(["a", "b", "--file", "f.json", "--since", "2025-01-01", "--max-pages", "10", "--max-tries", "2"]),
    { positional: ["a", "b"], files: ["f.json"], since: "2025-01-01", until: null, maxPages: 10, maxTries: 2 },
  );
  assert.deepEqual(parseArgs(["--file=x.txt", "--until=2026-09-24"]).files, ["x.txt"]);
  assert.equal(parseArgs(["--until=2026-09-24"]).until, "2026-09-24");
  assert.equal(parseArgs(["a"]).maxPages, 500);
  assert.equal(parseArgs(["a"]).maxTries, 3);
  assert.throws(() => parseArgs(["--max-pages", "0"]), /positive integer/);
  assert.throws(() => parseArgs(["--max-tries", "0"]), /positive integer/);
  assert.throws(() => parseArgs(["--nope"]), /Unknown option/);
});

test("handles come out of a json array, a keyed object, x-rows or a newline list", () => {
  assert.deepEqual(handlesFromFile('["a","b"]'), ["a", "b"]);
  assert.deepEqual(handlesFromFile('{"handles":["a","b"]}'), ["a", "b"]);
  assert.deepEqual(handlesFromFile('[{"x":"a"},{"x":"b"}]'), ["a", "b"]);
  assert.deepEqual(handlesFromFile("a\nb\n"), ["a", "b", ""]);
  assert.deepEqual(handlesFromFile('{"beta":[{"x":"a"}],"gamma":[{"x":"b"}]}'), ["a", "b"]);
});

test("collected handles are unique, valid and order-preserved", async () => {
  const got = await collectHandles(["@Alice", "bob", "alice", "bad handle"], []);
  assert.deepEqual(got, ["alice", "bob"]);
});

test("progress is reused for the same window and migrates the old {done} shape", () => {
  const saved = {
    since: "2025-09-24",
    until: "2026-09-24",
    exists: true,
    profile: { state: "exists", id: "44", lifetime: 900, created: "2021-01-01T00:00:00.000Z" },
    timeline_done: false,
    tweets: { count: 12, complete: true, tries: 1, oldest: "2025-09-25T00:00:00.000Z" },
    replies: { count: 3, complete: false, tries: 2, oldest: "2026-01-01T00:00:00.000Z" },
  };
  assert.deepEqual(openProgress(saved, "2025-09-24", "2026-09-24"), saved);
  // The earlier {count, done} shape is tolerated: done → complete, tries inferred, oldest/exists null.
  const legacy = openProgress(
    { since: "2025-09-24", until: "2026-09-24", tweets: { count: 5, done: true }, replies: { count: 0, done: false } },
    "2025-09-24",
    "2026-09-24",
  );
  assert.equal(legacy.exists, null);
  assert.equal(legacy.profile, null);
  assert.equal(legacy.timeline_done, false);
  assert.deepEqual(legacy.tweets, { count: 5, complete: true, tries: 1, oldest: null });
  assert.deepEqual(legacy.replies, { count: 0, complete: false, tries: 0, oldest: null });
  // A different window resets streams, existence, profile and the timeline flag.
  const reset = openProgress(saved, "2024-01-01", "2026-09-24");
  assert.equal(reset.exists, null);
  assert.equal(reset.profile, null);
  assert.deepEqual(reset.tweets, { count: 0, complete: false, tries: 0, oldest: null });
});

test("a stream is settled when complete or out of tries", () => {
  assert.equal(isSettled({ complete: true, tries: 0 }, 3), true);
  assert.equal(isSettled({ complete: false, tries: 3 }, 3), true);
  assert.equal(isSettled({ complete: false, tries: 2 }, 3), false);
  assert.equal(isSettled(undefined, 3), false);
});

test("work is every unsettled stream, users' streams kept adjacent", () => {
  const users = ["alice", "bob"];
  const progress = {
    alice: { tweets: { complete: true, tries: 1 }, replies: { complete: false, tries: 1 } },
    bob: { tweets: { complete: false, tries: 3 }, replies: { complete: false, tries: 0 } },
  };
  // alice/tweets is complete; bob/tweets is out of tries; both are skipped.
  assert.deepEqual(
    planWork(users, progress, 3).map((w) => `${w.user}/${w.stream}`),
    ["alice/replies", "bob/replies"],
  );
  assert.equal(planWork(users, progress, 3)[0].filter, STREAMS[1].filter);
});

test("a handle known not to exist is skipped entirely by planWork", () => {
  const progress = {
    dead: { exists: false, tweets: { complete: true, tries: 0 }, replies: { complete: true, tries: 0 } },
    live: { exists: true, tweets: { complete: false, tries: 0 }, replies: { complete: false, tries: 0 } },
  };
  assert.deepEqual(
    planWork(["dead", "live"], progress, 3).map((w) => `${w.user}/${w.stream}`),
    ["live/tweets", "live/replies"],
  );
});

test("resolution targets handles without a profile that are not already fully fetched", () => {
  const progress = {
    unknown: { tweets: { complete: false }, replies: { complete: false } }, // no profile → resolve
    done: { tweets: { complete: true }, replies: { complete: true } }, // fully fetched → skip
    profiled: { profile: { state: "exists" }, tweets: { complete: false }, replies: { complete: false } }, // has profile → skip
  };
  assert.deepEqual(planResolve(["unknown", "done", "profiled"], progress), ["unknown"]);
});

test("classifyUser distinguishes live, protected, never-posted and missing", () => {
  assert.deepEqual(
    classifyUser({ data: { user: { result: { __typename: "User", rest_id: "44", tweet_counts: { tweets: 900 }, core: { created_at: "Sat Sep 25 18:44:19 +0000 2021" } } } } }),
    { state: "exists", id: "44", lifetime: 900, created: "2021-09-25T18:44:19.000Z" },
  );
  assert.equal(classifyUser({ data: { user: { result: { __typename: "User", rest_id: "7", privacy: { protected: true } } } } }).state, "protected");
  assert.equal(classifyUser({ data: { user: { result: { __typename: "User", rest_id: "8", tweet_counts: { tweets: 0 } } } } }).state, "never_posted");
  assert.equal(classifyUser({ data: { user: {} } }).state, "missing"); // not found / renamed
  assert.equal(classifyUser({ data: { user: { result: { __typename: "UserUnavailable" } } } }).state, "missing"); // suspended
});

test("userUrl carries the screen_name in variables", () => {
  const url = userUrl("Vali_ETH");
  assert.match(url, /\/UserByScreenName\?/);
  assert.match(decodeURIComponent(url), /"screen_name":"Vali_ETH"/);
});

test("userTimelineUrl carries the user id and cursor", () => {
  const url = userTimelineUrl("44", "CURSOR");
  assert.match(url, /\/UserTweetsAndReplies\?/);
  assert.match(decodeURIComponent(url), /"userId":"44"/);
  assert.match(decodeURIComponent(url), /"cursor":"CURSOR"/);
});

const tlTweet = (id, uid, iso, over = {}) => ({
  tweet_results: {
    result: {
      rest_id: id,
      core: { user_results: { result: { rest_id: uid, legacy: { screen_name: "kol" } } } },
      legacy: { id_str: id, full_text: over.text ?? "hi", created_at: new Date(iso).toUTCString(), in_reply_to_status_id_str: over.reply ? "9" : undefined, retweeted_status_result: over.rt ? {} : undefined },
    },
  },
});

test("extractTweet flags replies and retweets and reads the author id", () => {
  const t = extractTweet(tlTweet("1", "44", "2026-01-01T00:00:00Z", { reply: true }).tweet_results.result);
  assert.equal(t.id, "1");
  assert.equal(t.author_id, "44");
  assert.equal(t.is_reply, true);
  assert.equal(t.is_retweet, false);
  assert.equal(extractTweet(tlTweet("2", "44", "2026-01-01T00:00:00Z", { rt: true }).tweet_results.result).is_retweet, true);
  assert.equal(extractTweet(tlTweet("3", "44", "2026-01-01T00:00:00Z", { text: "RT @x: hi" }).tweet_results.result).is_retweet, true);
});

test("parseUserTimeline keeps only the target user's posts across items, modules and the pin", () => {
  const body = {
    data: { user: { result: { timeline: { timeline: { instructions: [
      { type: "TimelinePinEntry", entry: { content: { itemContent: tlTweet("100", "44", "2026-02-01T00:00:00Z") } } },
      { type: "TimelineAddEntries", entries: [
        { entryId: "tweet-1", content: { itemContent: tlTweet("1", "44", "2026-01-03T00:00:00Z") } },
        // a conversation module: the user's reply (author 44) plus the tweet they replied to (author 99)
        { entryId: "profile-conversation-x", content: { items: [
          { item: { itemContent: tlTweet("99", "99", "2026-01-02T00:00:00Z") } },
          { item: { itemContent: tlTweet("2", "44", "2026-01-02T00:01:00Z", { reply: true }) } },
        ] } },
        { entryId: "cursor-bottom-1", content: { value: "NEXT" } },
      ] },
    ] } } } } },
  };
  const { posts, cursor } = parseUserTimeline(body, "44");
  assert.deepEqual(posts.map((p) => p.id).sort(), ["1", "100", "2"]); // 99 (other author) dropped
  assert.equal(cursor, "NEXT");
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
                        tweet_results: { result: { rest_id: id, legacy: { full_text: "x" } } },
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

// A full 20-id page, to exercise the paging loop past the first page.
const full = (start, cursor) =>
  page(Array.from({ length: 20 }, (_, i) => String(start + i)), cursor);

test("a stream ending on an empty page reports end (ambiguous until the caller weighs held posts)", async () => {
  const responses = [full(0, "c1"), page([], "c2")];
  const fetchPage = async () => responses.shift();
  const out = [];
  const status = await fetchStream({}, "q", 50, out, fetchPage, async () => {});
  assert.equal(out.length, 20);
  assert.equal(status, "end");
});

test("a stream ending on a short page is complete, deduplicating against what is already held", async () => {
  const responses = [full(0, "c1"), page(["0", "21"], "c2")]; // "0" is a dup of page 1
  const fetchPage = async () => responses.shift();
  const out = [];
  const status = await fetchStream({}, "q", 50, out, fetchPage, async () => {});
  assert.equal(out.length, 21);
  assert.equal(status, "complete");
});

test("a resumed stream keeps the batch it fetched even when getPage throws", async () => {
  const responses = [full(0, "c1"), () => { throw new Error("SearchTimeline failed after 6 attempts"); }];
  const fetchPage = async () => { const r = responses.shift(); return typeof r === "function" ? r() : r; };
  const out = [];
  await assert.rejects(fetchStream({}, "q", 50, out, fetchPage, async () => {}), /failed after 6/);
  assert.equal(out.length, 20); // the page fetched before the throw survives in the passed array
});

test("a passed-in batch seeds the dedup set so a resume does not re-add held posts", async () => {
  const responses = [page(["5", "99"], "c1")]; // "5" already held
  const fetchPage = async () => responses.shift();
  const out = [{ id: "5", created_at: "2026-01-01T00:00:00Z" }];
  const status = await fetchStream({}, "q", 50, out, fetchPage, async () => {});
  assert.deepEqual(out.map((t) => t.id), ["5", "99"]);
  assert.equal(status, "complete");
});

test("max-pages caps a runaway stream", async () => {
  let n = 0;
  const fetchPage = async () => full(20 * n++, "c" + n);
  const out = [];
  const status = await fetchStream({}, "q", 3, out, fetchPage, async () => {});
  assert.equal(out.length, 60);
  assert.equal(status, "capped");
});

test("streamComplete: short tail always done; results-end done only with data; cap never", () => {
  assert.equal(streamComplete("complete", false), true); // short page, even with 0 held → done
  assert.equal(streamComplete("end", true), true); // reached the floor with data → done
  assert.equal(streamComplete("end", false), false); // ended empty → gap (invisible / retry)
  assert.equal(streamComplete("capped", true), false); // more likely remains
});

test("roster objects merge every arbitrary array and reject malformed values", () => {
 assert.deepEqual(handlesFromFile('{"handles":["a"],"another":["b"]}'), ["a", "b"]);
 for (const text of ['null','123','{"a":"b"}']) assert.throws(() => handlesFromFile(text), /username arrays/);
});
