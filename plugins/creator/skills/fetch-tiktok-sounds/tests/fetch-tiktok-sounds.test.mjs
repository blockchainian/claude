// ABOUTME: Tests the pure parts of fetch-tiktok-sounds.mjs: arg parsing, the list request's pages, and
// ABOUTME: turning Studio's royalty-free music list into one row per sound.

import assert from "node:assert/strict";
import { test } from "node:test";

import { pageBodies, parseArgs, soundRows } from "../scripts/fetch-tiktok-sounds.mjs";

test("parseArgs takes an optional --username, --count, --headed and --with-sound", () => {
  assert.deepEqual(parseArgs([]), { username: null, count: 20, headed: false, withSound: false });
  assert.deepEqual(parseArgs(["--username", "@me", "--count", "45", "--headed", "--with-sound"]), { username: "me", count: 45, headed: true, withSound: true });
  assert.throws(() => parseArgs(["--with-sound"]), /--with-sound needs --headed/);
  assert.throws(() => parseArgs(["--count", "0"]), /--count/);
  assert.throws(() => parseArgs(["--count", "many"]), /--count/);
  assert.throws(() => parseArgs(["--bogus"]), /Unknown option --bogus/);
});

test("pageBodies asks for the hot list a page at a time until the count is covered", () => {
  assert.deepEqual(pageBodies(45), [
    { page_num: 1, page_size: 20, source: 1, order_by: 1, music_type: 1 },
    { page_num: 2, page_size: 20, source: 1, order_by: 1, music_type: 1 },
    { page_num: 3, page_size: 20, source: 1, order_by: 1, music_type: 1 },
  ]);
  assert.equal(pageBodies(20).length, 1);
});

test("soundRows keeps the id, title, author, length and number of posts using each sound", () => {
  const list = [{ id_str: "7603363008859047972", id: 7603363008859048000, title: "Comedy Corridor", author: "Finley Reed", duration: 109, user_count: 1522180 }];
  assert.deepEqual(soundRows(list), [{ id: "7603363008859047972", title: "Comedy Corridor", author: "Finley Reed", seconds: 109, posts: 1522180 }]);
});
