---
name: capture-window
description: Claude-only. Capture one macOS app's front window to a PNG and look at it, leaving every other window on the user's screen untouched. Use in Claude Code whenever you need to see a desktop app — VS Code, iTerm, a native app — to check a theme, a layout or what the user is pointing at. NOT for iOS screens (mobile:ios-take-screenshot) or web pages (claude-in-chrome).
---

# Capture a window

This skill is Claude-only. It addresses Claude's computer-use capture behavior;
it is intentionally excluded from the Codex marketplace.

Claude's computer-use screenshots hide every app outside the session allowlist, so the
user's browser and other windows vanish. This captures only the named app's
window with `screencapture -l`; nothing else is hidden, raised or focused.

Set `SKILL_DIR` to the absolute directory containing the `SKILL.md` the host loaded for this skill.

```bash
"$SKILL_DIR/scripts/capture-window" "<app name>" "<scratchpad>/<name>.png"
```

- `<app name>` is the window owner as macOS reports it: `Code` for VS Code, `iTerm2`, `Finder`.
- The front-most on-screen window of that app is captured; the script prints the PNG path.
- Write the PNG to the session scratchpad, then Read it to look.
- Exit 1 with `no on-screen window` means the app has no visible window (minimized, other Space, or wrong name).
