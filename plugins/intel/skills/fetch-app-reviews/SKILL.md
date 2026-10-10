---
name: fetch-app-reviews
description: Fetch every App Store written review of an iOS app across all storefronts into ~/.local/state/intel/reviews/<name>.json, rotating a residential-proxy exit per request, resumable per storefront — or of every app in ~/.local/state/intel/reviews/apps.json. Use when asked to fetch / 抓 / 拉 an app's App Store reviews or refresh the reviews dataset. NOT for reading the reviews (analyze-appstore-reviews) and NOT for X mentions (fetch-x-mentions).
---

# Fetch App Store reviews

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env`, then fill in only the values needed by the skills you use. The CLI loads that file without replacing variables already exported in the shell.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_STATE_DIR` | State root; default ~/.local/state/intel, with reviews under reviews/ | No | `~/.config/intel/.env` |
| `RESIDENTIAL_PROXY_URL` | Rotating residential proxy | Yes | `~/.config/intel/.env` |

Each request opens a fresh proxy connection to rotate the exit IP.

## Run

Archives are written under `INTEL_STATE_DIR/reviews/`, independent of the working directory.

One app:

```
node \
  "$SKILL_DIR/scripts/fetch-app-reviews.mjs" <appleId> [name]
```

- `appleId` is the numeric App Store id, e.g. 6741115427.
- `name` is the output slug; omitted, it is derived from the store name.

Every app in `~/.local/state/intel/reviews/apps.json` (the crypto-app leaderboard: rank, name, appId, ratings), bottom rank first:

```
node \
  "$SKILL_DIR/scripts/fetch-all-app-reviews.mjs"
```

It runs for a long time, so run it in the background.

Both are resumable: rerun the same command. Per-storefront completion is kept in the output
file, so a retry only re-fetches storefronts that have not confirmed an end. The all-apps run
retries incomplete apps for up to 6 passes.

## Output

`~/.local/state/intel/reviews/<name>.json`: `{ appId, appName, updatedAt, complete, countriesDone, count, reviews }`.
`complete` is true only when every storefront reached a confirmed end.
