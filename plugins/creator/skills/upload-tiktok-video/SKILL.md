---
name: upload-tiktok-video
description: Post one mp4 to the TikTok account the secrets-manager skill logged in — its earliest imported active account — through TikTok Studio's upload page in that account's own Camoufox profile on its ISP slot, with a caption, who can see it, the AI-generated label, the promotion disclosure and a commercial-library sound; --headed shows and screen-records the browser to debug a broken step; the posted video's id is found and the post logged to posts.jsonl. Use when asked to upload / post / 上传 / 发 a video to TikTok, or to test the upload flow. NOT for logging the account in or its status (secrets-manager), NOT for the video's numbers afterwards (fetch-tiktok-stats) and NOT for the official Content Posting API.
---

# Upload TikTok video

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the Creator plugin's `.env.example` to `~/.config/creator/.env`, the one file Claude Code,
Codex and every worktree read; values already in the shell take precedence.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ISP_PROXY_URL` | ISP pool matching the account's login pool | Yes | ~/.config/creator/.env |
| `BROWSER_DISPLAY` | Display for headed browser windows | Optional | ~/.config/creator/.env |
| `CREATOR_STATE_DIR` | State root; default ~/.local/state/creator, with each account's posts.jsonl and stats.jsonl under tiktok/<username>/ | Optional | ~/.config/creator/.env |
| `CREATOR_DATA_DIR` | Data root; default ~/.local/share/creator, with each account's screen recordings under tiktok/<username>/recordings/ | Optional | ~/.config/creator/.env |
| `SECRETS_STATE_DIR` | Existing Secrets Manager account/profile directory; default ~/.local/state/secrets-manager | Optional | ~/.config/creator/.env |

Account credentials, sessions and profiles stay in Secrets Manager; Creator's configuration is
separate.

## Setup

The script needs Node.js 22.13+ with `node:sqlite` and the Camoufox browser, which the secrets
plugin's setup fetches. Install the npm dependencies once; all four creator skills share them:

```sh
(cd "$SKILL_DIR/scripts" && npm ci)
```

Creator only reads the secrets-manager store, which Claude Code and Codex share and which holds the
TikTok account rows, ISP slots and browser profiles. If an account is missing, provision or log it
in with secrets-manager; installing creator on another host neither creates nor migrates accounts.
Screen recording needs macOS's Screen Recording permission for the terminal that runs `swift`.

## Run

```sh
node "$SKILL_DIR/scripts/upload-tiktok-video.mjs" <video.mp4> \
  [--username <name>] [--caption <text>] [--visibility everyone|friends|only-me] [--aigc] \
  [--promotion your-brand|branded-content|your-brand,branded-content] [--sound <id>] \
  [--headed [--with-sound]]
```

- `--username` posts as that account of the store, which must be `active`.
- `--caption` is the description, hashtags included (`"my words #tag #other"`); it defaults to empty.
- `--visibility` sets who can see the post and defaults to `everyone`.
- `--aigc` turns on TikTok's "AI-generated content" label, under "Show more".
- `--promotion` turns on "Disclose post content", under "Show more", and ticks `your-brand` (the
  account promotes itself), `branded-content` (a paid partnership with another brand; posting it
  agrees to TikTok's Branded Content Policy), or both, comma-separated.
- `--sound` adds the commercial-library sound with that id, which fetch-tiktok-sounds lists. It plays
  under the video's own audio, and the post links to it as its sound.
- `--headed` shows the browser window and screen-records it; without it the browser is hidden. Use
  it when a step fails, because the selectors break whenever TikTok Studio's page changes.
- `--with-sound`, with `--headed`, unmutes the browser, which Playwright mutes, so you can hear the video.

Give the run a ten-minute deadline. A post takes from about 20 seconds to a few minutes, most of it
spent waiting for the video to show on the profile. On timeout, keep the output and report the
process and its state, and never retry automatically, because the post may already be live.

## The account

Without `--username`, the script posts as the store's earliest imported (`created_at`) `active`
TikTok account with an ISP slot. It opens the account exactly as `login tiktok` did: the same profile
directory (`profiles/<username>` in the store), stored fingerprint and ISP slot, in the English locale. A profile that is no longer
signed in stops the run with `run the secrets-manager skill's login tiktok`; log it in there and run
again. Nothing else may have the profile open at the same time, such as a fetch-tiktok-mentions run
on the same account or a `login tiktok`.

## What it does

The script opens TikTok Studio's upload page, chooses the file and waits for TikTok's "Uploaded".
Whenever an overlay blocks a step, and only then, it closes the "new editing features" tour and
accepts the offer to turn on automatic content checks (music copyright and For You eligibility, an
account setting); both appear only on a profile's first few visits. With `--sound` it reads the
sound's title by its id, searches that title in the editor's Sounds panel, presses "+" on the result
with the same id, since titles repeat, and saves the edit. It then replaces the prefilled
description (the file name) with the caption, typed key by key, and sets who can see the post, the
AI-generated label and the promotion disclosure, each of which turns on at once without
confirmation. It presses Post, confirms "Post now" if TikTok asks, and waits to leave the upload
page. Finally it opens the account's profile and looks for the new video by its create time, through
the same page-signed API calls fetch-tiktok-stats uses, every 5 seconds (randomly shifted by up to
1.5 seconds) for up to five minutes.

## Output

Each post appends one line to `tiktok/<username>/posts.jsonl` under the state root:
`{ at, username, file, caption, visibility, aigc, promotion, sound, videoId, url, recording }`.
`videoId` is null when the video had not reached the profile in time.

A `--headed` run, failed ones included, saves its screen recording as
`tiktok/<username>/recordings/upload-<ts>.mov` under the data root. When a step fails, rerun with
`--headed` and watch the recording first.

## Not here

Scheduling and the Content Posting API are not supported.
