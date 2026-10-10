---
name: fetch-app-reviews
description: Fetch every App Store written review of an iOS app across all storefronts into the intel state folder, rotating a residential-proxy exit per request, resumable per storefront — or of every app in the reviews app list. Use when asked to fetch / 抓 / 拉 an app's App Store reviews or refresh the reviews dataset. NOT for reading the reviews (analyze-appstore-reviews) and NOT for X mentions (fetch-x-mentions).
---

# Fetch app reviews

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the intel plugin’s `.env.example` to `~/.config/intel/.env` and fill in only the values the
skills you use need. The scripts load that file without replacing variables already exported in the
shell.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_STATE_DIR` | State root; reviews go under `reviews/`; default `~/.local/state/intel` | No | `~/.config/intel/.env` |
| `RESIDENTIAL_PROXY_URL` | Rotating residential proxy | Yes | `~/.config/intel/.env` |

## Setup

Install the scripts' dependencies once:

```sh
npm install --prefix "$SKILL_DIR/scripts"
```

## Run

To fetch one app, pass its numeric App Store id (for example 6741115427) and, optionally, the output
name; without a name, the script derives one from the app's store name.

```sh
node "$SKILL_DIR/scripts/fetch-app-reviews.mjs" <appleId> [name]
```

To fetch every app in `reviews/apps.json` under the state root, a ranked app list with `rank`,
`name`, `appId` and ratings, run the all-apps script. It goes from the bottom rank up and takes a
long time, so run it in the background.

```sh
node "$SKILL_DIR/scripts/fetch-all-app-reviews.mjs"
```

Both write under `reviews/` in the state root, whatever the working directory. Both resume when you
rerun the same command: the output file records which storefronts reached a confirmed end, and a
rerun fetches only the rest. The all-apps script itself retries incomplete apps for up to 6 passes.

## Output

Each app goes to `reviews/<name>.json` under the state root as
`{ appId, appName, updatedAt, complete, countriesDone, count, reviews }`. `complete` is true only
when every storefront reached a confirmed end; otherwise rerun the command.
