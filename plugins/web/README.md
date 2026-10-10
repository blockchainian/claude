# web

Inspect and drive web apps from Claude Code or Codex: run a persistent headless Chromium from the
shell, find memory leaks by diffing V8 heap snapshots, and check a built page against a design
reference.

## Install

For Codex installation, see
[Codex installation](../../README.md#codex).

In Claude Code:

```
/plugin marketplace add blockchainian/claude
/plugin install web@blockchainian
```

`browse` builds itself on first use, so it needs `bun`, `git`, network access and a Playwright
Chromium. `heap-snapshot-leaks` needs Node 22+ and Google Chrome, and `check-web-design` needs `uv`.
Optional settings go in `~/.config/web/.env`, starting from this plugin's `.env.example`.

## Skills

| Skill | What it does |
|---|---|
| `browse` | Drive a persistent headless Chromium with the `browse` CLI — navigate, interact, run JS, read console and network logs, screenshot — with one tab per client for parallel runs |
| `heap-snapshot-leaks` | Capture two heap snapshots around a repeated action and diff them into a ranked report of the constructors that grew and the DOM nodes left detached |
| `check-web-design` | Diff a built web page against a design reference and report the off-by colours, positions, and missing/extra elements |

How to use each one is in its `skills/<skill>/SKILL.md`. `browse` is
[gstack](https://github.com/garrytan/gstack)'s browser CLI (MIT) with this plugin's patches applied;
`skills/browse/AGENTS.md` covers developing, checking and upgrading those patches.

Before adding a skill, check that claude-in-chrome (driving the user's own Chrome) and `cloudflare:web-perf`
(page performance) do not already cover it.

`check-web-design`'s `check_design.py` and its test are byte-identical with the `mobile` plugin's
`check-mobile-design`, so edit both copies; a repo test fails if they drift. The two SKILL.md files
also share the sections "Reading `diff.json`", "Tolerances" and "Requirements", differing only in the
platform nouns; no test covers that prose, so change both together.

## Tests

Run `npm run test:web` from the marketplace root. Tests that open a visible Chromium window are
skipped unless `BROWSE_HEADED_TESTS=1`.
