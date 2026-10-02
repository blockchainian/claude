// ABOUTME: Tests for fetch-x-posts.mjs: argument parsing, the SearchTimeline row mapping and the
// ABOUTME: shared account state (oldest-first pick, 3-second spacing, pauses).
import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseArgs, rowsOf, openState, claim, pause, GAP_MS } from "../scripts/fetch-x-posts.mjs";

const state = (names) => openState(join(mkdtempSync(join(tmpdir(), "fetch-x-posts-")), "state.sqlite"), names);

test("parseArgs defaults to 40 latest posts", () => {
  assert.deepEqual(parseArgs(["from:a min_faves:10"]), { query: "from:a min_faves:10", limit: 40, product: "Latest" });
});

test("parseArgs reads --limit and --top", () => {
  assert.deepEqual(parseArgs(["--top", "q", "--limit", "20"]), { query: "q", limit: 20, product: "Top" });
});

test("parseArgs rejects an unknown option, a bad limit and a missing query", () => {
  assert.throws(() => parseArgs(["q", "--since", "x"]), /unknown option --since/);
  assert.throws(() => parseArgs(["q", "--limit", "0"]), /--limit/);
  assert.throws(() => parseArgs([]), /usage/);
});

const tweetEntry = (entryId, result) => ({ entryId, content: { itemContent: { tweet_results: { result } } } });
const page = (entries) => ({
  data: { search_by_raw_query: { search_timeline: { timeline: { instructions: [{ entries }] } } } },
});

test("rowsOf maps a post, preferring the long-post text and decoding entities", () => {
  const body = page([
    tweetEntry("tweet-1", {
      rest_id: "1",
      core: { user_results: { result: { core: { screen_name: "zachxbt", name: "ZachXBT" } } } },
      views: { count: "1200" },
      note_tweet: { note_tweet_results: { result: { text: "the whole long post" } } },
      legacy: {
        created_at: "Wed Oct 01 12:00:00 +0000 2025",
        full_text: "the whole long…",
        favorite_count: 5,
        retweet_count: 2,
        reply_count: 1,
        quoted_status_permalink: { expanded: "https://twitter.com/b/status/9" },
        in_reply_to_status_id_str: "7",
      },
    }),
    tweetEntry("tweet-2", {
      __typename: "TweetWithVisibilityResults",
      tweet: {
        rest_id: "2",
        core: { user_results: { result: { legacy: { screen_name: "old", name: "Old Shape" } } } },
        legacy: { created_at: "Wed Oct 01 13:00:00 +0000 2025", full_text: "a &amp; b &lt;c&gt;" },
      },
    }),
    tweetEntry("promoted-tweet-3", { rest_id: "3", legacy: { full_text: "ad" } }),
    { entryId: "cursor-bottom-0", content: { value: "NEXT" } },
  ]);
  const { rows, cursor } = rowsOf(body);
  assert.equal(cursor, "NEXT");
  assert.deepEqual(rows, [
    {
      id: "1",
      url: "https://x.com/zachxbt/status/1",
      created_at: "2025-10-01T12:00:00.000Z",
      user: "zachxbt",
      name: "ZachXBT",
      text: "the whole long post",
      likes: 5,
      retweets: 2,
      replies: 1,
      views: 1200,
      quoted: "https://x.com/b/status/9",
      in_reply_to: "7",
    },
    {
      id: "2",
      url: "https://x.com/old/status/2",
      created_at: "2025-10-01T13:00:00.000Z",
      user: "old",
      name: "Old Shape",
      text: "a & b <c>",
      likes: 0,
      retweets: 0,
      replies: 0,
      views: null,
      quoted: null,
      in_reply_to: null,
    },
  ]);
});

test("rowsOf reads posts grouped in a module and the cursor of a replace instruction", () => {
  const item = { item: { itemContent: { tweet_results: { result: { rest_id: "5", legacy: { full_text: "x" } } } } } };
  const body = {
    data: {
      search_by_raw_query: {
        search_timeline: {
          timeline: {
            instructions: [
              { entries: [{ entryId: "tweet-module", content: { items: [item] } }] },
              { entry: { entryId: "cursor-bottom-1", content: { value: "C2" } } },
            ],
          },
        },
      },
    },
  };
  const { rows, cursor } = rowsOf(body);
  assert.deepEqual(rows.map((r) => r.id), ["5"]);
  assert.equal(cursor, "C2");
});

test("rowsOf returns null for a body without a search result", () => {
  assert.equal(rowsOf({ errors: [{ code: 88 }] }), null);
});

test("claim takes the account used longest ago and then moves on", () => {
  const db = state(["a", "b", "c"]);
  const pool = ["a", "b", "c"];
  const first = [claim(db, pool, 1000), claim(db, pool, 1001), claim(db, pool, 1002)];
  assert.deepEqual(first.map((c) => c.username).sort(), ["a", "b", "c"]);
  assert.ok(first.every((c, i) => c.at === 1000 + i));
  assert.equal(claim(db, pool, 1003).username, first[0].username);
});

test("claim keeps the gap between two requests on one account", () => {
  const db = state(["a"]);
  assert.deepEqual(claim(db, ["a"], 1000), { username: "a", at: 1000 });
  assert.deepEqual(claim(db, ["a"], 1500), { username: "a", at: 1000 + GAP_MS });
  assert.deepEqual(claim(db, ["a"], 1600), { username: "a", at: 1000 + 2 * GAP_MS });
  assert.deepEqual(claim(db, ["a"], 90000), { username: "a", at: 90000 });
});

test("claim skips a paused account and takes it again after its reset", () => {
  const db = state(["a", "b"]);
  pause(db, "a", 60000);
  assert.equal(claim(db, ["a", "b"], 1000).username, "b");
  assert.equal(claim(db, ["a", "b"], 1001).username, "b");
  assert.equal(claim(db, ["a", "b"], 60000).username, "a");
});

test("claim reports the earliest reset when every account is paused", () => {
  const db = state(["a", "b"]);
  pause(db, "a", 60000);
  pause(db, "b", 50000, "403");
  assert.deepEqual(claim(db, ["a", "b"], 1000), { resetAt: 50000 });
});

test("claim ignores accounts of the state that are no longer in the pool", () => {
  const db = state(["a", "b"]);
  assert.equal(claim(db, ["b"], 1000).username, "b");
  assert.equal(claim(db, ["b"], 1001).username, "b");
});
