# Decisions: <feature>

<!-- This file is for the user, read once before /feature:ship launches. No agent reads it. Write
it clear, succinct and accurate — optimised for fast reading, lowest cognitive load, no fact lost.
Bullets over prose, plain words, NO code anchors or line numbers (those live in plan.md). If the
user can grasp the whole page in one scan and still veto correctly, it is done. -->

<!-- Every decision's normative outcome (what the code must do) already lives in plan.md's
workstream steps and Constraints. This file carries only the why, the rejected alternatives, and
what a veto would change — never repeat the implementation steps here. -->

<!-- Do NOT write a `Gate: passed` line — /feature:plan inserts it as this file's first line once the
user has cleared the gate, and /feature:ship refuses to launch without it. -->

## Summary

<!-- Two or three plain sentences: what this change does and why, in the user's language. No
jargon, no file paths. -->

## Decisions to veto

<!-- One bullet per real design decision the user could reasonably overrule. Each: what was chosen,
what a veto changes, and the alternatives rejected with the one-line reason each was dropped. Omit
decisions with no live alternative. -->

- **<decision>** — chosen: <one line>. Veto means: <what changes>. Rejected: <alt> (<why>).

## Risks

<!-- What could go wrong that the user should know before ship. One line each; empty is allowed —
write `None identified.` rather than inventing risks. -->

- <risk>

## Open questions

<!-- Anything still needing the user's judgment before ship — including any of problem.md's open
questions the planner carried here instead of turning into a `QUESTION:` line. Empty is the normal
case; write `None.` -->

- <question>
