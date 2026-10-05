---
name: open-tiktok-account
description: Open a secrets-manager TikTok account's own Camoufox profile in a shown browser window, signed in as that account on its ISP slot, at TikTok Studio or any tiktok.com page, so a person can look around — private (only-me) posts, Studio analytics, settings — optionally with sound; the run ends when the window is closed. Use when asked to open / 打开 a TikTok account, or to see or hear a post only the account can see. NOT for posting (upload-tiktok-video), NOT for numbers (fetch-tiktok-stats) and NOT for logging the account in (secrets-manager).
---

# Open a TikTok account

```
node "${CLAUDE_PLUGIN_ROOT}/skills/open-tiktok-account/scripts/open-tiktok-account.mjs" \
  [--username <name>] [--url <url>] [--with-sound]
```

- `--username`: the account of the store, in any status: a `restricted` one opens too, to see
  TikTok's ban notice or appeal. Default the account upload-tiktok-video posts as.
- `--url`: the tiktok.com page to open. Default TikTok Studio's home.
- `--with-sound`: unmute the browser (Playwright mutes it), to hear the videos.

Launch it in the background: it holds until the person closes the window. The window is always
shown, so there is no `--headed`, and nothing is recorded. The profile is the one `login tiktok`
signed in, opened as that same device and IP; a logged-out profile is said so in the log. Nothing
else may have the profile open at the same time, so close the window before uploading or listing
sounds as that account.
