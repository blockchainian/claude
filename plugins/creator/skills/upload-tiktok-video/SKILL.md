---
name: upload-tiktok-video
description: Post one mp4 to the TikTok account the secrets-manager skill logged in — its earliest imported active account — through TikTok Studio's upload page in that account's own Camoufox profile on its ISP slot, with a caption, who can see it, the AI-generated label, the promotion disclosure and a commercial-library sound; --headed shows and screen-records the browser to debug a broken step; the posted video's id is found and the post logged to posts.jsonl. Use when asked to upload / post / 上传 / 发 a video to TikTok, or to test the upload flow. NOT for logging the account in or its status (secrets-manager), NOT for the video's numbers afterwards (fetch-tiktok-stats) and NOT for the official Content Posting API.
---

# Upload a TikTok video

## Environment Variables

Copy the Creator plugin’s `.env.example` to `~/.config/creator/.env`; both hosts and all worktrees use this file. Existing shell values take precedence.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ISP_PROXY_URL` | ISP pool matching the account’s login pool | Yes | ~/.config/creator/.env |
| `BROWSER_DISPLAY` | Display for headed browser windows | Optional | ~/.config/creator/.env |
| `CREATOR_STATE_DIR` | State root; default ~/.local/state/creator, with each account's posts.jsonl and stats.jsonl under tiktok/<username>/ | Optional | ~/.config/creator/.env |
| `CREATOR_DATA_DIR` | Data root; default ~/.local/share/creator, with each account's screen recordings under tiktok/<username>/recordings/ | Optional | ~/.config/creator/.env |
| `SECRETS_STATE_DIR` | Existing Secrets Manager account/profile directory | Optional | ~/.config/creator/.env |

Account credentials, sessions and profiles remain in Secrets Manager; Creator configuration is separate.

## Runtime and paths

Works in Claude Code and Codex. Set `SKILL_DIR` to the absolute directory containing the
`SKILL.md` that the host loaded for this skill; replace the example value below with that actual
path. Do not use the project's working directory or a host-specific plugin-root variable.
Keep the full creator plugin installed: all four skills use its
`upload-tiktok-video/scripts/tiktok-session.mjs` runtime and dependencies.

Requires Node.js 22.13+ with `node:sqlite`, the upload-tiktok-video runtime's npm dependencies,
and the Camoufox browser installed by secrets-manager. The existing secrets-manager store is
shared across both hosts at `~/.local/state/secrets-manager` (`SECRETS_STATE_DIR` explicitly
overrides it), including TikTok account rows, ISP slots and browser profiles.
Configure `ISP_PROXY_URL` in `~/.config/creator/.env` or the process environment. Creator reads this
store; use secrets-manager to provision or log in an account if it is missing. Installing
creator in another host does not create or migrate accounts.

## Run

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
node "$SKILL_DIR/scripts/upload-tiktok-video.mjs" <video.mp4> \
  [--username <name>] [--caption <text>] [--visibility everyone|friends|only-me] [--aigc] \
  [--promotion your-brand|branded-content|your-brand,branded-content] [--sound <id>] \
  [--headed [--with-sound]]
```

- `--username`: post as that account of the store; it must be `active`. Default below.
- `--caption`: the description, hashtags included (`"my words #tag #other"`). Default empty.
- `--visibility`: who can see the post. Default `everyone`.
- `--aigc`: turn on TikTok's "AI-generated content" label (under "Show more").
- `--promotion`: turn on "Disclose post content" (under "Show more") and tick `your-brand` (the
  account promotes itself), `branded-content` (a paid partnership with another brand; posting it
  agrees to TikTok's Branded Content Policy), or both, comma-separated.
- `--sound`: add the commercial-library sound with that id (fetch-tiktok-sounds lists them). It
  plays under the video's own audio, and the post links to it as its sound.
- `--headed`: show the browser window and screen-record it. Use it when a step fails: TikTok
  Studio's page changes, and the selectors with it. Without it the browser is hidden.
- `--with-sound`: with `--headed`, unmute the browser (Playwright mutes it) to hear the video.

Run through the host's shell execution tool and retain its process/session handle and output.
If it continues in the background, use the host's completion notification or a wait on that
handle, rather than repeatedly rereading a log. Set a ten-minute deadline before launch;
a post takes from about 20 s to a few minutes, most of it waiting for the video to show on the
profile. On timeout, preserve output and report the process handle and its state; do not
retry a possibly completed post automatically.

## The account

`--username`, else the secrets-manager store's earliest imported (`created_at`) `active` TikTok
account with an ISP slot. It is opened exactly as `login tiktok` opened it: the same profile directory
(`<state>/profiles/<username>`), the same stored fingerprint, the same ISP slot, English locale.
The store is only read. A profile that is no longer signed in stops the run with
`run the secrets-manager skill's login tiktok`; log it in there and run again. Nothing else may
have the profile open at the same time (a fetch-tiktok-mentions run using the same account, a
`login tiktok`).

## What it does

1. Opens `tiktokstudio/upload`; with `--headed`, shows the window and starts recording it.
2. Chooses the file and waits for TikTok's "Uploaded".
3. Whenever an overlay gets in the way of a step (never waiting for one otherwise): closes the
   "new editing features" tour, and accepts the offer to turn on automatic content checks (music
   copyright and For You eligibility, an account setting). Both show only on a profile's first
   few visits.
4. With `--sound`: reads the sound's title by its id, opens the editor's Sounds panel, searches the
   title, presses "+" on the result with that id (titles repeat) and saves the edit.
5. Replaces the prefilled description (the file name) with the caption, typed key by key.
6. Sets who can see the post, the AI-generated label and the promotion disclosure when asked
   (each turns on at once, no confirmation).
7. Presses Post, confirming "Post now" if TikTok asks, and waits to leave the upload page.
8. Opens the account's profile and finds the new video by its create time, through the same
   page-signed API calls fetch-tiktok-stats uses, every 5 s, moved randomly by up to 1.5 s, for up to five minutes.

## Output

One directory per account:

- `~/.local/state/creator/tiktok/<username>/posts.jsonl` (`CREATOR_STATE_DIR` overrides the root): one line per post,
  `{ at, username, file, caption, visibility, aigc, promotion, sound, videoId, url, recording }`.
  `videoId` is null when the video had not reached the profile in time.
- `~/.local/share/creator/tiktok/<username>/recordings/upload-<ts>.mov` (`CREATOR_DATA_DIR` overrides the root): the screen recording of a `--headed` run, also on failure. When a
  step fails, rerun with `--headed` and watch it first.

## Setup

All four creator skills share this install:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
(cd "$SKILL_DIR/scripts" && npm ci)
```

Camoufox is fetched by the secrets plugin's setup. `ISP_PROXY_URL` comes from `~/.config/creator/.env`.
Recording needs macOS's Screen Recording permission for the terminal running `swift`.

## Not here

Scheduling and the Content Posting API.
