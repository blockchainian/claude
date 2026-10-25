#!/usr/bin/env node
// ABOUTME: Reduces a web article (a blog post, not a course page) to its title, author line and body with Mozilla
// ABOUTME: Readability, dropping tags, dates, avatars, reading time, share bars, subscription boxes and footers.
//
// Usage: readable.mjs <page.html> <url> <out.html>
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";
import { Readability } from "@mozilla/readability";

const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** The article in a page as a standalone HTML document, or null when Readability finds none. */
export function readable(page, url) {
  const article = new Readability(new JSDOM(page, { url }).window.document).parse();
  if (!article || !article.content) return null;
  // A page title often carries the site's name ("… | Tor Project", "ImperialViolet - …"); the longest of the page's
  // own headings (or heading links) that the title contains does not.
  const doc = new JSDOM(page).window.document;
  const heading = [...doc.querySelectorAll("h1, h2, h3, h1 a, h2 a, h3 a")].map((h) => h.textContent.replace(/\s+/g, " ").trim())
    .filter((h) => h && article.title.includes(h)).sort((a, b) => b.length - a.length)[0];
  const title = heading || article.title.trim();
  const byline = article.byline ? `<p class="byline">${esc(article.byline.trim())}</p>` : "";
  return {
    title,
    html: `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>${esc(title)}</title></head>` +
      `<body><h1>${esc(title)}</h1>${byline}${article.content}</body></html>`,
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  const [src, url, out] = process.argv.slice(2);
  const result = readable(fs.readFileSync(src, "utf8"), url);
  if (!result) {
    console.error(`readable.mjs: no article found in ${src}`);
    process.exit(1);
  }
  fs.writeFileSync(out, result.html);
  console.log(`article: ${result.title}`);
}
