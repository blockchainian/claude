// ABOUTME: Tests the pure part of open-tiktok-account.mjs: arg parsing, with TikTok Studio's home as the
// ABOUTME: default page and the window always shown; a named account opens in any status.

import assert from "node:assert/strict";
import { test } from "node:test";

import { parseArgs } from "../scripts/open-tiktok-account.mjs";

test("parseArgs defaults to Studio's home, muted, the default account", () => {
  assert.deepEqual(parseArgs([]), { username: null, url: "https://www.tiktok.com/tiktokstudio", withSound: false });
});

test("parseArgs takes --username, --url and --with-sound", () => {
  assert.deepEqual(parseArgs(["--username", "@me", "--url", "https://www.tiktok.com/@me/video/1", "--with-sound"]), {
    username: "me",
    url: "https://www.tiktok.com/@me/video/1",
    withSound: true,
  });
});

test("parseArgs refuses --headed (the window is always shown), a url off TikTok and unknown options", () => {
  assert.throws(() => parseArgs(["--headed"]), /Unknown option --headed/);
  assert.throws(() => parseArgs(["--url", "https://example.com"]), /--url/);
  assert.throws(() => parseArgs(["--bogus"]), /Unknown option --bogus/);
});
