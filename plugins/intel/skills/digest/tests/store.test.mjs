// ABOUTME: Tests the highlights store CLI: save, list, search and take-aways against a scratch
// ABOUTME: HIGHLIGHTS_DIR, plus the default location and the frontmatter parser.
import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, symlinkSync, writeFileSync, existsSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseFrontmatter, resolveRoot } from "../scripts/store.mjs";

const script = fileURLToPath(new URL("../scripts/store.mjs", import.meta.url));

function makeStore() {
  const root = mkdtempSync(join(tmpdir(), "store-test-"));
  const run = (...args) => {
    const res = spawnSync(process.execPath, [script, ...args], {
      env: { ...process.env, HIGHLIGHTS_DIR: root },
      encoding: "utf-8",
    });
    return { code: res.status, stdout: res.stdout, stderr: res.stderr };
  };
  const draft = (name, text) => {
    const path = join(root, name);
    mkdirSync(join(path, ".."), { recursive: true });
    writeFileSync(path, text, "utf-8");
    return path;
  };
  return { root, items: join(root, "items"), run, draft };
}

const ARTICLE = `---
title: Why X Wins
url: https://site.com/posts/why-x-wins
slug: why-x-wins
source: Example Blog
topics: [x]
---
# Why X Wins

## Point
- a point
`;

describe("store location", () => {
  test("defaults to ~/Documents/highlights", () => {
    assert.equal(resolveRoot({}), join(homedir(), "Documents", "highlights"));
  });

  test("HIGHLIGHTS_DIR overrides the default", () => {
    assert.equal(resolveRoot({ HIGHLIGHTS_DIR: "/tmp/elsewhere/" }), "/tmp/elsewhere");
  });
});

describe("parseFrontmatter", () => {
  test("reads keys, strips quotes and returns the body", () => {
    const [meta, body] = parseFrontmatter('---\ntitle: "Quoted"\nurl: https://a.b\n---\nbody\n');
    assert.deepEqual(meta, { title: "Quoted", url: "https://a.b" });
    assert.equal(body, "body\n");
  });

  test("text without frontmatter is left alone", () => {
    assert.deepEqual(parseFrontmatter("# plain\n"), [{}, "# plain\n"]);
  });
});

describe("save", () => {
  test("an article draft (no show) saves and is stamped with a date", () => {
    const s = makeStore();
    const res = s.run("save", s.draft("draft.md", ARTICLE));
    const saved = join(s.items, "why-x-wins.md");
    assert.equal(res.code, 0, res.stderr);
    assert.ok(existsSync(saved));
    assert.equal(res.stdout, `saved: ${saved}\nindex: ${join(s.root, "index.md")} (1 items)\n`);
    const [meta] = parseFrontmatter(readFileSync(saved, "utf-8"));
    assert.match(meta.saved, /^\d{4}-\d{2}-\d{2}$/);
  });

  test("a bare title+url draft saves", () => {
    const s = makeStore();
    const res = s.run("save", s.draft("d2.md", "---\ntitle: Bare\nurl: https://site.com/bare\nslug: bare\n---\n# Bare\n"));
    assert.equal(res.code, 0, res.stderr);
    assert.ok(existsSync(join(s.items, "bare.md")));
  });

  test("a draft missing its url is refused", () => {
    const s = makeStore();
    const res = s.run("save", s.draft("d3.md", "---\ntitle: NoUrl\nslug: nourl\n---\n# NoUrl\n"));
    assert.equal(res.code, 1);
    assert.equal(res.stderr, "draft frontmatter is missing: url\n");
    assert.ok(!existsSync(join(s.items, "nourl.md")));
  });

  test("a missing draft path is refused", () => {
    const s = makeStore();
    const res = s.run("save", join(s.root, "nope.md"));
    assert.equal(res.code, 1);
    assert.equal(res.stderr, `no such draft: ${join(s.root, "nope.md")}\n`);
  });

  test("saving the same url again is a no-op", () => {
    const s = makeStore();
    const draft = s.draft("draft.md", ARTICLE);
    s.run("save", draft);
    const res = s.run("save", draft);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /^already saved: .*why-x-wins\.md\n\(no-op; add take-aways/);
  });

  test("a different item on the same slug lands beside it as -2", () => {
    const s = makeStore();
    s.run("save", s.draft("a.md", ARTICLE));
    const res = s.run("save", s.draft("b.md", "---\ntitle: Other\nurl: https://site.com/other\nslug: why-x-wins\n---\n# Other\n"));
    assert.equal(res.code, 0, res.stderr);
    assert.ok(existsSync(join(s.items, "why-x-wins-2.md")));
  });

  test("the slug falls back to the draft's directory name", () => {
    const s = makeStore();
    const res = s.run("save", s.draft("My Episode/draft.md", "---\ntitle: T\nurl: https://site.com/t\n---\n# T\n"));
    assert.equal(res.code, 0, res.stderr);
    assert.ok(existsSync(join(s.items, "my-episode.md")));
  });
});

describe("list and search", () => {
  test("list and search report an empty store", () => {
    const s = makeStore();
    const list = s.run("list");
    assert.equal(list.code, 0);
    assert.equal(list.stdout, `nothing saved yet (${join(s.root, "index.md")} does not exist)\n`);
    const search = s.run("search", "x");
    assert.equal(search.code, 1);
    assert.equal(search.stderr, `nothing saved yet (${s.items} does not exist)\n`);
  });

  test("list prints the rebuilt index", () => {
    const s = makeStore();
    s.run("save", s.draft("draft.md", ARTICLE));
    const res = s.run("list");
    assert.equal(res.code, 0);
    assert.match(res.stdout, /^# Highlights\n\n- \d{4}-\d{2}-\d{2} — \*\*Example Blog\*\* — \[Why X Wins\]\(items\/why-x-wins\.md\) — https:\/\/site\.com\/posts\/why-x-wins\n\n$/);
  });

  test("search is a case-insensitive regex over the whole file", () => {
    const s = makeStore();
    s.run("save", s.draft("draft.md", ARTICLE));
    const res = s.run("search", "example BLOG");
    assert.equal(res.code, 0);
    assert.equal(res.stdout, [
      "",
      "=== Example Blog — Why X Wins",
      `    ${join(s.items, "why-x-wins.md")}`,
      "    https://site.com/posts/why-x-wins",
      "    6: source: Example Blog",
      "",
      "1 item(s) matched 'example BLOG'",
      "",
    ].join("\n"));
    assert.match(s.run("search", "zzz").stdout, /\n0 item\(s\) matched 'zzz'\n$/);
  });

  test("an invalid regex falls back to a literal search", () => {
    const s = makeStore();
    s.run("save", s.draft("draft.md", ARTICLE));
    const res = s.run("search", "(x");
    assert.equal(res.code, 0);
    assert.match(res.stdout, /^not a valid regex \(.*\) - searching for it literally\n/);
    assert.match(res.stdout, /0 item\(s\) matched '\(x'\n$/);
  });
});

describe("takeaway", () => {
  test("add and revise edit a Take-aways section at the top of the stored file", () => {
    const s = makeStore();
    s.run("save", s.draft("draft.md", ARTICLE));
    const saved = join(s.items, "why-x-wins.md");

    assert.equal(s.run("takeaway", saved, "--list").stdout, "(no take-aways yet)\n");

    const added = s.run("takeaway", saved, "--add", "keep the thesis, not the timeline");
    assert.equal(added.code, 0, added.stderr);
    assert.equal(added.stdout, "take-aways for why-x-wins.md:\n1. keep the thesis, not the timeline\n");

    const revised = s.run("takeaway", saved, "--revise", "1", "keep the thesis");
    assert.equal(revised.code, 0, revised.stderr);
    assert.equal(revised.stdout, "take-aways for why-x-wins.md:\n1. keep the thesis\n");

    const text = readFileSync(saved, "utf-8");
    assert.match(text, /\n# Why X Wins\n\n## Take-aways\n\n- keep the thesis\n\n## Point\n- a point\n$/);
  });

  test("revise out of range is refused", () => {
    const s = makeStore();
    s.run("save", s.draft("draft.md", ARTICLE));
    const res = s.run("takeaway", join(s.items, "why-x-wins.md"), "--revise", "3", "x");
    assert.equal(res.code, 1);
    assert.equal(res.stderr, "--revise needs an item number in 1..0\n");
  });

  test("a file outside items/ is not a stored item", () => {
    const s = makeStore();
    const draft = s.draft("draft.md", ARTICLE);
    const res = s.run("takeaway", draft, "--add", "x");
    assert.equal(res.code, 1);
    assert.equal(res.stderr, `not a stored item: ${draft}\nsave the item first; take-aways attach to the stored file.\n`);
  });
});

describe("usage", () => {
  test("no arguments or an unknown command prints usage and exits 1", () => {
    const s = makeStore();
    for (const args of [[], ["bogus"], ["save"], ["takeaway"], ["takeaway", "x.md", "--revise", "n", "t"]]) {
      const res = s.run(...args);
      assert.equal(res.code, 1, args.join(" "));
      assert.match(res.stderr, /^usage: store\.mjs save <draft\.md> \| search <query> \| list\n/);
      assert.equal(res.stdout, "");
    }
  });

  test("the script runs when called through a symlink to its folder, as an installed plugin is", () => {
    const linked = join(mkdtempSync(join(tmpdir(), "store-link-")), "scripts");
    symlinkSync(join(script, ".."), linked);
    const res = spawnSync(process.execPath, [join(linked, "store.mjs")], { encoding: "utf-8" });
    assert.match(res.stderr, /^usage: store\.mjs/);
  });
});
