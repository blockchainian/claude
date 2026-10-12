// ABOUTME: Tests readable.mjs on a real blog post: the article body is kept, the site's chrome and the site name in
// ABOUTME: the title are not.

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { readable } from "../scripts/readable.mjs";

const page = fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "blog-article.html"), "utf8");
const result = readable(page, "https://blog.torproject.org/top-changes-tor-2004-design-part-3/");

test("title drops the site name", () => assert.equal(result.title, "Top changes in Tor since the 2004 design paper (Part 3)"));
test("article body is kept", () => assert.match(result.html, /Nick Mathewson and Steven Murdoch/));
test("site chrome is dropped", () => {
  assert.doesNotMatch(result.html, /donate\.torproject\.org/);
  assert.doesNotMatch(result.html, /Newsletter/);
});
