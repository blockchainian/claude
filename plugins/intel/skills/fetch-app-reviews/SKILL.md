---
name: fetch-app-reviews
description: Fetch every App Store written review of an iOS app across all storefronts into docs/intel/reviews/<name>.json, rotating a residential-proxy exit per request, resumable per storefront — or of every app in docs/intel/reviews/apps.json. Use when asked to fetch / 抓 / 拉 an app's App Store reviews or refresh the reviews dataset. NOT for reading the reviews (analyze-appstore-reviews) and NOT for X mentions (fetch-x-mentions).
---

# Fetch App Store reviews

## Runtime and paths

Works in Claude Code and Codex. Resolve `SKILL_DIR` from the absolute directory of
this loaded `SKILL.md`, not the working directory or a host-specific environment variable:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

Repeat this assignment and any `S`, `T` or `U` assignments used below in every shell call;
shell variables may not persist between calls. If the loaded path is unavailable, stop
and report it. Keep the full intel plugin installed: sibling skills share scripts.
Run archive commands from the repository that owns the archive; configuration and
account stores are shared between hosts and are not migrated by installing intel.

For finite long-running commands, choose a deadline before launch and retain the process
handle and output. In Claude Code use `run_in_background` and its completion notification;
in Codex use the shell tool's process/session handle and wait for completion. Subagents
must await their own commands before returning. Do not repeatedly poll logs or assume a
background completion wakes either host. On timeout, preserve diagnostics and report the
process state before retrying. Use the current host's image/file tools to inspect artifacts.

Run from the repo root.

One app:

```
node \
  "$SKILL_DIR/scripts/fetch-app-reviews.mjs" <appleId> [name]
```

- `appleId` is the numeric App Store id, e.g. 6741115427.
- `name` is the output slug; omitted, it is derived from the store name.

Every app in `docs/intel/reviews/apps.json` (the crypto-app leaderboard: rank, name, appId, ratings), bottom rank first:

```
node \
  "$SKILL_DIR/scripts/fetch-all-app-reviews.mjs"
```

Both are resumable: rerun the same command. Per-storefront completion is kept in the output
file, so a retry only re-fetches storefronts that have not confirmed an end. The all-apps run
retries incomplete apps for up to 6 passes.

## Output

`docs/intel/reviews/<name>.json`: `{ appId, appName, updatedAt, complete, countriesDone, count, reviews }`.
`complete` is true only when every storefront reached a confirmed end.

## Environment Variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env`, then fill in only the values needed by the skills you use. The CLI loads that file without replacing variables already exported in the shell.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `RESIDENTIAL_PROXY_URL` | Rotating residential proxy | Yes, unless HTTPS_PROXY is set | `~/.config/intel/.env` |
| `HTTPS_PROXY` | Proxy when RESIDENTIAL_PROXY_URL is unset | Alternative to RESIDENTIAL_PROXY_URL | `~/.config/intel/.env` |

Each request opens a fresh proxy connection to rotate the exit IP.
