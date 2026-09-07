# `ios-take-screenshot` with two simulators booted

Date: 2026-09-07. Plugin build-ios-apps 0.6.6. Verified on iPhone 16 (A, 1179x2556) and
iPhone 16 Pro (B, 1206x2622), both iOS 18.3, ChadWallet 62 on both.

## What works

- The skill's scripts refuse to guess: `discover_ios_setup.py --target simulator` exits 1
  naming both UDIDs, `find_ios_app.sh --simulator booted` exits 2. Raw
  `xcrun simctl io booted` silently picks one (it picked A) with no warning.
- With `--device <B>` discovery exits 0, and a full Portfolio capture on B following the
  document unaided came out correct: 5 slices, every seam spliced, 1206x6154.

## The mismatch, reproduced

Session default set to A, `simctl` captures from B. The swipe scrolled A; B never moved.
`frame_diff.py`: `mean_abs_diff 0.0, scrolled_px 0`. Stitcher on the two identical slices:
`all_spliced: true`, sticky 786/786, output 1206x1836 — shorter than one screen, and it
looks like an ordinary page fragment. Nothing in the tooling notices. The doc's
"one slice is the whole screen" rule then relabels the bug as a finished capture.

## Status

Fixed in build-ios-apps 0.6.7, in this order: a per-simulator claim (`claim_simulator.py`,
exit 3 names the holder so the later agent waits or aborts) with `capture_slice.sh`
refusing unclaimed simulators and `booted`; the stitcher refuses identical slices; the doc
reads defaults back, compares `artifacts.simulatorId`, says per-call targeting is
impossible and how to pick with `--device`; discovery reports a shutdown UDID as such and
emits the simulator name; `frame_diff.py` refuses a size mismatch; `IOS_SCREENSHOT_DIR`
shown inline; dev-launcher guidance. Not fixable here: the tools' `nextSteps` hints (6).

## Issues, most silent first

1. **Session defaults are shared by every agent in a Claude Code session and never read
   back.** Each session launches its own XcodeBuildMCP process (five were running at once
   here) and nothing is persisted to disk, so sessions do not see each other's defaults —
   but subagents of one session share its server, and this run started with defaults
   already pointing at A, set by an earlier agent in the same session. Doc: after
   `session_set_defaults`, call `session_show_defaults` and compare with the discovered
   UDID; one simulator per session, never two agents capturing in the same session.
2. **A mismatch is indistinguishable from a non-scrolling screen.** Every XcodeBuildMCP
   response echoes the simulator it hit (`artifacts.simulatorId`; snapshots add `udid`).
   Doc: compare that against the capture UDID before taking the "does not scroll" branch.
3. **The stitcher accepts two identical slices.** It crops most of each as chrome and
   emits a green verdict on an image shorter than a slice. Script: refuse when
   `vs_previous_diff` is ~0 on every seam, or when the output is shorter than one slice.
4. **The doc never says how to pick a simulator when two are booted.** It says to read
   `selectedSimulator.udid`, which is null; `--device` appears only in the script's error
   string and, in the doc, only for the real-device app finder. Doc.
5. **A shutdown UDID gets the wrong message.** `--device <not booted>` says
   "2 simulators booted; pass --device with one of their UDIDs". Script: say that UDID is
   not booted.
6. **`nextSteps` hints from the tools advertise a `simulatorId` parameter** on tap/swipe
   that no schema has; `launch_app_sim`, `snapshot_ui`, `swipe`, `screenshot`, `tap` all
   target the default only. Tool limitation; doc should say per-call targeting is
   impossible.
7. **`discover_ios_setup.py` emits only `simulatorId`**, so a stale `simulatorName` from
   an earlier default survives beside the new id. Script: emit the name too.
8. **`frame_diff.py` accepts slices of different sizes** (cross-simulator) by cropping to
   the common size; the stitcher refuses. Script: refuse on a size mismatch.
9. **`IOS_SCREENSHOT_DIR` is presented as an environment variable** but each command runs
   in a fresh shell, so it has to be passed inline. Doc.
10. **No guidance for an app that resumes on a dev launcher** (Expo) instead of itself.
    Doc.

Related: `frame_diff.py` reports 30% chrome on each edge for identical frames (detector
cap), which is cosmetic once issue 3 is fixed.
