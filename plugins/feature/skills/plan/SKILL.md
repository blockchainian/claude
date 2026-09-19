---
name: plan
description: >
  Run the feature pipeline's planning phase: spawn the planner on a grounded
  problem.md, then interactively approve its decisions, open questions and
  risks before ship. Use after /feature:ground and before /feature:ship —
  "plan X", "run the planner", "review the plan's decisions". This is the
  feature pipeline's plan step, NOT Claude Code's built-in /plan (plan mode).
  NOT for grounding, not for shipping, and not for a change small enough to
  hold in one turn.
---

# Plan — spawn the planner, then gate its decisions with the user

The planner writes the plan; this skill makes its judgment calls the user's, fast. The planner is a
background agent and cannot prompt the user, so the interactive gate lives here in the main loop.
The skill never edits the *content* of `plan.md` or `decisions.md` — the planner owns both and
re-runs the checkers; this skill only asks and routes the answers back. Its one write is the gate
stamp it inserts as the first line of `decisions.md` when the review settles (step 4), which is a
record, not content.

## 1. Spawn the planner

The input is a grounded `problem.md` (from `/feature:ground`). If there is none, stop and say to
ground first. Spawn the `planner` agent (`feature:planner`) with the problem.md path and
end the turn; its completion re-invokes you. It writes `plan.md` (the agent-facing spec) and
`decisions.md` (the human gate) beside `problem.md`, and reports the two paths, the plan's word
count and the three checkers' exit codes. If any checker is non-zero, or the plan carries
`QUESTION:` lines, resolve those first (feed answers back by `SendMessage` to the
same planner — it is idle, not dead) before gating.

## 2. Gate `decisions.md` with the user

Read `decisions.md`. Present its three sections through `AskUserQuestion`, at most 4 questions per
call, each header ≤12 chars. The tool cannot pre-select or pre-check options — the first option
carries `(Recommended)`, and multi-select starts empty. Skip any section that is empty.

- **Decisions to veto** — one question per decision.
  - A yes/no decision: options `Approve` (first, Recommended) and `Reject`. The always-present
    `Other` box is the user typing feedback.
  - A decision with real alternatives: options are the choices themselves — the chosen one first
    and Recommended, then each rejected alternative as its own option. `Other` is free-text.
- **Open questions** — one question each: options `Find out` (Recommended) and `Irrelevant`; the
  `Other` box is the user typing the answer.
- **Risks** — one `multiSelect: true` question: "Check every risk you accept." List each risk as an
  option. All checked on submit = no objection. Any left unchecked = the user does not accept it.

## 3. Apply the answers

Route each answer; the plan changes only through the planner.

- **Approve / every risk checked / no open questions** → nothing to change.
- **Reject, a picked alternative, or typed feedback** → the decision changes. `SendMessage` the
  planner the exact deltas (which decision, the new choice or the feedback verbatim) and have it
  revise `plan.md` + `decisions.md` and re-run the checkers.
- **Open question — `Find out`** → you investigate in the main loop (grep, read, a probe) and hand
  the found answer to the planner to fold in. Never guess; if you cannot settle it, put it back to
  the user.
- **Open question — `Irrelevant`** → tell the planner to drop it.
- **Open question — typed answer** → hand the answer to the planner to fold in.
- **A risk left unchecked** → discuss it and its mitigation with the user. If the mitigation changes
  the plan (a new constraint or step), the planner makes the change; if the user accepts the risk
  after discussing, record that and move on.

## 4. Settle and hand off

When the planner has revised, re-read `decisions.md` and re-gate **only** the decisions, questions or
risks that changed — not the ones already approved. Cap this at two revision rounds; a decision that
is still contested after two rounds goes to the user as a plain question, outside the gate. When
every decision is approved, every open question resolved, and every risk accepted, stamp the gate:
insert as the first line of `decisions.md` a line reading `Gate: passed <date> — all decisions approved, open
questions resolved, risks accepted` (a risk the user accepted only after discussion is still
accepted). This line is what `/feature:ship` checks before launch. Then say the plan is launch-ready
and the next step is `/feature:ship <plan.md>`.

Non-interactive runs (no user to ask): skip the gate, leave `decisions.md` for manual review, and
report that the plan is planned but unreviewed — do not auto-approve, and do not launch ship.
