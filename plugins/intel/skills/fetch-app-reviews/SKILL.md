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

## Config

`~/.config/intel/.env`: `RESIDENTIAL_PROXY_URL`, the rotating
residential proxy. A new exit IP is handed out per connection, so the script opens a fresh
proxy connection per request; that is what defeats Apple's per-IP throttling.

Archive paths are relative to the working directory, run from the repo root that owns the archive.

`~/.config/intel/.env` loads automatically without replacing existing environment values.
Keys: `X_BEARER`, `X_SEARCH_QUERY_ID`, `X_USER_QUERY_ID`, `X_USER_TWEETS_QID`,
`X_TID_VERIFICATION`, `X_TID_FRAME`, `X_TID_ROW`, `X_TID_INDICES`,
`RESIDENTIAL_PROXY_URL`, `X_PROXY_URLS`, `ISP_PROXY_URL`, `ISP_PROXY_COUNT`.
Only keys needed by this script are required. Missing required keys report this config path.

Setup: `npm install --prefix "$SKILL_DIR/scripts"`.
