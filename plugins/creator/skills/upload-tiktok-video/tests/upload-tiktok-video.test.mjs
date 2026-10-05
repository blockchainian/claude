// ABOUTME: Tests the pure parts of upload-tiktok-video.mjs: arg parsing and finding the posted video
// ABOUTME: among the account's videos, and the jittered wait between looks.

import assert from "node:assert/strict";
import { test } from "node:test";

import { findPosted, findDelay, parseArgs } from "../scripts/upload-tiktok-video.mjs";

test("parseArgs takes the file, the caption and the post settings", () => {
  assert.deepEqual(
    parseArgs(["a.mp4", "--caption", "hi #tag", "--visibility", "only-me", "--aigc", "--promotion", "your-brand,branded-content", "--username", "@me"]),
    {
      file: "a.mp4",
      username: "me",
      caption: "hi #tag",
      visibility: "only-me",
      aigc: true,
      promotion: ["your-brand", "branded-content"],
    },
  );
});

test("parseArgs takes one kind of promotion", () => {
  assert.deepEqual(parseArgs(["a.mp4", "--promotion", "branded-content"]).promotion, ["branded-content"]);
});

test("parseArgs defaults to an empty caption, everyone, no AI label and no promotion", () => {
  assert.deepEqual(parseArgs(["a.mp4"]), { file: "a.mp4", username: null, caption: "", visibility: "everyone", aigc: false, promotion: [] });
});

test("parseArgs refuses a missing file, an unknown visibility or promotion and an unknown option", () => {
  assert.throws(() => parseArgs([]), /Usage/);
  assert.throws(() => parseArgs(["a.mp4", "--visibility", "public"]), /--visibility/);
  assert.throws(() => parseArgs(["a.mp4", "--bogus"]), /Unknown option --bogus/);
  assert.throws(() => parseArgs(["a.mp4", "--promotion", "ad"]), /--promotion/);
  assert.throws(() => parseArgs(["a.mp4", "--ai-generated"]), /Unknown option --ai-generated/);
});

test("findPosted takes the newest video created since the post began", () => {
  const items = [
    { id: "new", createTime: 1000 },
    { id: "old", createTime: 900 },
  ];
  assert.equal(findPosted(items, 995).id, "new");
  assert.equal(findPosted(items, 1001), null);
});

test("findDelay is five seconds give or take a random second and a half, in whole ms", () => {
  assert.equal(findDelay(() => 0), 3500);
  assert.equal(findDelay(() => 0.5), 5000);
  assert.equal(findDelay(() => 0.9999), 6500);
});
