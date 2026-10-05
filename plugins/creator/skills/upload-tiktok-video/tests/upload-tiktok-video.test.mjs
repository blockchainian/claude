// ABOUTME: Tests the pure parts of upload-tiktok-video.mjs: arg parsing and finding the posted video
// ABOUTME: among the account's videos, and the jittered wait between looks.

import assert from "node:assert/strict";
import { test } from "node:test";

import { findPosted, findDelay, parseArgs, soundIndex } from "../scripts/upload-tiktok-video.mjs";

test("parseArgs takes the file, the caption and the post settings", () => {
  assert.deepEqual(
    parseArgs(["a.mp4", "--caption", "hi #tag", "--visibility", "only-me", "--aigc", "--promotion", "your-brand,branded-content", "--sound", "7603363008859047972", "--username", "@me"]),
    {
      file: "a.mp4",
      username: "me",
      caption: "hi #tag",
      visibility: "only-me",
      aigc: true,
      promotion: ["your-brand", "branded-content"],
      sound: "7603363008859047972",
    },
  );
});

test("parseArgs takes one kind of promotion", () => {
  assert.deepEqual(parseArgs(["a.mp4", "--promotion", "branded-content"]).promotion, ["branded-content"]);
});

test("parseArgs defaults to an empty caption, everyone, no AI label, no promotion and no sound", () => {
  assert.deepEqual(parseArgs(["a.mp4"]), { file: "a.mp4", username: null, caption: "", visibility: "everyone", aigc: false, promotion: [], sound: null });
});

test("parseArgs refuses a missing file, an unknown visibility or promotion, a sound that is not an id, and an unknown option", () => {
  assert.throws(() => parseArgs([]), /Usage/);
  assert.throws(() => parseArgs(["a.mp4", "--visibility", "public"]), /--visibility/);
  assert.throws(() => parseArgs(["a.mp4", "--bogus"]), /Unknown option --bogus/);
  assert.throws(() => parseArgs(["a.mp4", "--promotion", "ad"]), /--promotion/);
  assert.throws(() => parseArgs(["a.mp4", "--ai-generated"]), /Unknown option --ai-generated/);
  assert.throws(() => parseArgs(["a.mp4", "--sound", "Comedy Corridor"]), /--sound/);
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

test("soundIndex finds a sound's row among the Sounds panel's search results by its id", () => {
  const results = { data: [{ music_info: { id_str: "1", title: "A" } }, { music_info: { id_str: "2", title: "A" } }] };
  assert.equal(soundIndex(results, "2"), 1);
  assert.equal(soundIndex(results, "3"), -1);
  assert.equal(soundIndex({}, "1"), -1);
});
