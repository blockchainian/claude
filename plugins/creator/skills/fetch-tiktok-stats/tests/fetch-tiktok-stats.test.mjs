// ABOUTME: Tests the pure parts of fetch-tiktok-stats.mjs: arg parsing and turning a page of TikTok's
// ABOUTME: items into one stats row per video.

import assert from "node:assert/strict";
import { test } from "node:test";

import { parseArgs, statsRows } from "../scripts/fetch-tiktok-stats.mjs";

test("parseArgs takes an optional --username", () => {
  assert.deepEqual(parseArgs([]), { username: null });
  assert.deepEqual(parseArgs(["--username", "@someone"]), { username: "someone" });
  assert.throws(() => parseArgs(["--bogus"]), /Unknown option --bogus/);
});

test("statsRows keeps one row per video with its counters", () => {
  const items = [
    { id: "2", createTime: 1791157500, stats: { playCount: 10, diggCount: 2, commentCount: 1, shareCount: 0, collectCount: 3 } },
    { id: "1", createTime: 1791157000, statsV2: { playCount: "7" }, stats: { playCount: 7 } },
  ];
  assert.deepEqual(statsRows("me", items, "2026-10-05T00:00:00.000Z"), [
    { at: "2026-10-05T00:00:00.000Z", username: "me", videoId: "2", createTime: 1791157500, playCount: 10, diggCount: 2, commentCount: 1, shareCount: 0, collectCount: 3 },
    { at: "2026-10-05T00:00:00.000Z", username: "me", videoId: "1", createTime: 1791157000, playCount: 7, diggCount: 0, commentCount: 0, shareCount: 0, collectCount: 0 },
  ]);
});
