# Outcome: <feature>

<!-- Written by the orchestrator at phase end (ship step 9). This file is for
the user and for memory — the phase's record, and the only place the shipped outcome is written;
plan.md stays input-only. Write it clear, succinct and accurate, optimised for fast reading and no
fact lost: bullets, plain words, no code anchors. -->

Shipped <date>. <!-- or: Not shipped — <one line why>. -->

## Delivered

<!-- One line per workstream in the plan. Each: the workstream id, delivered or not, and the test or
probe that proved it. A workstream whose test never ran or went red is not delivered and the feature
did not fully ship. -->

- `<workstream id>` — delivered · <test or probe name>

## Code review

<!-- One bullet per must-fix finding (local and cloud), nits dropped. Each: a tldr of the issue, a
tldr of the fix, and the commit SHA that fixed it. Write `No must-fix findings.` if the review was
clean. -->

- **<issue in one line>** → fixed: <fix in one line> · `<sha>`

## Deploy

- **PR:** #<n> — <title>, merged as `<sha>`.
- **Staging:** <origin> `<version>` (tree `<sha>`) — <verified / skipped, one line why>.
- **Production:** <origin> `<sha>` — <live-confirmed / one line>.

## Live checks

<!-- The plan's Live checks, run against production. Command → result, one line each. -->

- `<command>` → <result>

## Follow-ups

<!-- Work deliberately left out of this ship: deferred items, non-blocking observations, lessons.
One line each. A genuinely actionable follow-up seeds the next plan — name it. Empty is fine;
write `None.` -->

- <follow-up>
