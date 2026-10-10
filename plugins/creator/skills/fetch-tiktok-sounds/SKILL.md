---
name: fetch-tiktok-sounds
description: List the hottest sounds of TikTok's commercial (royalty-free) music library — the list TikTok Studio's "Royalty-free sounds" page shows — with the ids upload-tiktok-video's --sound takes, read in the secrets-manager account's own signed-in Camoufox profile. Use when asked which music / 配乐 / 商用音乐 / 热歌 to put on a TikTok post, or for a sound id to upload with. NOT for adding the sound to a post (upload-tiktok-video --sound), NOT for video stats (fetch-tiktok-stats).
---

# Fetch TikTok sounds

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The script
imports `scripts/tiktok-session.mjs` from the sibling `upload-tiktok-video` skill, so keep the whole
creator plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the creator plugin's `.env.example` to `~/.config/creator/.env`, the one file Claude Code,
Codex and every worktree read; a value already set in the shell wins.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ISP_PROXY_URL` | ISP pool matching the account's login pool | Yes | ~/.config/creator/.env |
| `BROWSER_DISPLAY` | Display for headed browser windows | Optional | ~/.config/creator/.env |
| `CREATOR_STATE_DIR` | State root; default ~/.local/state/creator, with each account's posts.jsonl and stats.jsonl under tiktok/<username>/ | Optional | ~/.config/creator/.env |
| `CREATOR_DATA_DIR` | Data root; default ~/.local/share/creator, with each account's screen recordings under tiktok/<username>/recordings/ | Optional | ~/.config/creator/.env |
| `SECRETS_STATE_DIR` | Existing secrets-manager account and profile directory; default ~/.local/state/secrets-manager | Optional | ~/.config/creator/.env |

Account credentials, sessions and profiles stay in secrets-manager, apart from the creator
configuration.

## Setup

The script needs Node.js 22.13+ with `node:sqlite`, the npm dependencies of the
upload-tiktok-video runtime, and the Camoufox browser that secrets-manager installs. It reads the
existing secrets-manager store, which Claude Code and Codex share and which holds the TikTok account
rows, ISP slots and browser profiles. If the account is missing, provision it or log it in with
secrets-manager; installing creator on another host neither creates nor migrates accounts.

Install the dependencies once for the installed creator plugin; all four creator skills share this
install:

```sh
(cd "$SKILL_DIR/../upload-tiktok-video/scripts" && npm ci)
```

## Run

```sh
node "$SKILL_DIR/scripts/fetch-tiktok-sounds.mjs" [--username <name>] [--count <n>] [--headed [--with-sound]]
```

- `--username` reads the list as that account of the store; by default it is the account
  upload-tiktok-video posts as.
- `--count` sets how many sounds to list, hottest first; the default is 20.
- `--headed` shows the browser window and screen-records it to
  `<username>/recordings/sounds-<ts>.mov` under the data root, to debug a read that stopped working.
- `--with-sound`, together with `--headed`, unmutes the browser (Playwright mutes it) so the sounds
  play on the page.

The script prints one line per sound:

```
7603363008859047972  Comedy Corridor — Finley Reed  1:49  1522060 posts
```

The first column is the id to pass to `upload-tiktok-video --sound`, and "posts" counts the posts
that use the sound.

## How it works

The list is Studio's own (`creator/music/unlimited/list`, about 770,000 sounds sorted by hot), so it
needs a signed-in account. The script opens the account's Camoufox profile on its ISP slot at
`tiktokstudio/sound-library` and fetches the list inside that page, 20 sounds a page. Nothing else may
have the profile open while it runs.

Every sound on this list is cleared for use in any post on TikTok, promotional ones included, but
the clearance does not extend to other platforms.
