---
name: capture-window
description: Claude-only. Capture one macOS app's front window to a PNG and look at it, leaving every other window on the user's screen untouched. Use in Claude Code whenever you need to see a desktop app — VS Code, iTerm, a native app — to check a theme, a layout or what the user is pointing at. NOT for iOS screens (mobile:ios-take-screenshot) or web pages (claude-in-chrome).
---

# Capture window

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md`.

## Capture

A computer-use screenshot hides every app outside the session allowlist, so the user's browser and
other windows vanish from the screen. This skill captures only the named app's window with
`screencapture -l`, and hides, raises or focuses nothing else.

```bash
"$SKILL_DIR/scripts/capture-window" "<app name>" "<scratchpad>/<name>.png"
```

Give the app name as macOS reports the window owner: `Code` for VS Code, `iTerm2`, `Finder`. The
script captures that app's front-most on-screen window and prints the PNG path. Write the PNG to the
session scratchpad, then Read it to look.

If the script exits 1 with `no on-screen window`, the app has no visible window: it is minimized, on
another Space, or the name is wrong.
