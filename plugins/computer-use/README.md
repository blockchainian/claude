# computer-use

See the user's macOS desktop from Claude Code without disturbing it.

## Skills

| Skill | What it does |
|---|---|
| `capture-window` | Capture one app's front window to a PNG; every other window stays visible and in place |

## How it works

`capture-window` asks CoreGraphics (through a short `swift` script) for the
front-most on-screen window of the named app, then runs
`screencapture -x -o -l<window id>`. Unlike a computer-use screenshot, nothing
outside that window is hidden, raised or focused. Needs macOS with the Xcode
command-line tools (`swift`) and Screen Recording permission for the terminal.
