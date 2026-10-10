---
name: retro
description: >
  Find the biggest wastes of effort in a finished orchestration session and
  route each fix back into the workflow it came from. Reads that session's
  transcript as evidence, ranks wastes by real token cost, and proposes fixes
  to ship and the plan template. Use when an orchestration session that spawned
  agents has finished, run in a SEPARATE session with the finished session's
  name: "/feature:retro <session-name>". NOT in the session being analysed (a
  session grading itself inherits the blind spot that caused the waste), and
  not for a one-turn task with nothing to rank.
---

# Retro

A session wastes effort because its model of the world was wrong: it did not think to check the
gate, or to read the existing client. That same context, asked to critique itself, inherits the blind
spot and misreports its own cost, so the retro runs in a fresh session that audits the finished
session's transcript. The transcript's stated reasoning is a claim to verify, never a justification
for a deviation.

Run it after an orchestration session with a spawn ledger worth ranking, such as a multi-workstream
ship or a sweep, and skip it for a session that spawned nothing. Diagnosis and fixes are two gates:
write the diagnosis and have it confirmed before touching anything.

The human record is `retros/<date>-<session>/retro.md` under the output root. Write it there and
never into a repo, because the analysed session's worktree is often deleted once it finishes. The
machine records, `retros/<date>-<session>/retro.json` and the `retros/fixes.jsonl` ledger, live under
the state root, because they cannot be rebuilt once the transcripts age out.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `FEATURE_OUTPUT_DIR` | Output root; `retro.md` goes under `retros/`; default `~/Documents` | Optional | Shell environment |
| `FEATURE_STATE_DIR` | State root; `retro.json` and `fixes.jsonl` go under `retros/`; default `~/.local/state/feature` | Optional | Shell environment |

## Extract the evidence

```
${CLAUDE_PLUGIN_ROOT}/skills/retro/scripts/extract.mjs <session-name>
```

The script joins each spawn's `tool_use.id` to the `toolUseId` in `subagents/<agent>.meta.json` and
reads each child transcript's own token usage, so the numbers need no instrumentation. It prints the
orchestrator's own cost, the spawn ledger (spawns counted by `subagent_type`) and every subagent
ranked by billable tokens with its model, agent type and workstream label. It also prints the paths
of the orchestrator transcript and the `subagents/` directory, which you grep to trace the costly
turns.

## Trace the costly turns

Token cost shows where the effort went; the transcript says why. For each costly cluster, such as a
re-run workstream, a repeated probe or a long tail, grep the orchestrator transcript for the gate
result, the error and the re-do. One rule decides attribution:

> A failed gate is not proof of bad code. Ask whether the **code** was wrong or the **plan/gate**
> was wrong. A run where every workstream "failed its check" but all wrote correct code is a plan
> defect, not an agent defect.

## Classify each waste

Put each waste on one axis. Every finding carries a token cost and a pointer to the fact or rule that
would have prevented it; drop any finding that lacks either.

- **Axis A, knowable-fact miss.** The answer already existed in memory or the repo, such as a memory
  that warned of the gate or an existing client that held the auth recipe. The fix goes into the
  AGENTS.md or skill the planning turn reads, so the check happens up front. Verifying that the
  answer existed is a read-only repo check; delegate that fan-out to Explore agents
  (`model: "sonnet"`).
- **Axis B, workflow or topology deviation.** The session did not run the shape ship prescribes,
  for example UX work done by `general-purpose` agents instead of `ux-implementer` and `ux-verifier`,
  or hand-driving instead of probe-first. The spawn ledger shows this directly. Mark each one a
  **lapse** (ship was clear and discipline failed, so add a ship guardrail) or a **signal** (the
  shape did not fit, so change the workflow). The fix goes into `/feature:ship`.
- **Axis C, plan defect.** The plan encoded a gate, spec, scope or criterion that could not hold and
  failed only downstream: a check that cannot pass on the baseline, a value contradicting existing
  code, an under-scoped workstream whose gate was too narrow to catch cross-file breakage, an
  unsatisfiable success criterion, or missing de-risk sequencing (no pilot, no probe-first). The fix
  goes into `skills/ship/minimal-plan-template.md` and ship's plan check.

Keep an **Inherent (not waste)** bucket for test retargeting, legitimate exploration and gate fixes
the plan could not have avoided. Being honest about what was not waste is what makes the ranking
credible.

## Account for the codex workstream

Codex cost is not in the Claude transcript, but `extract.mjs` recovers it from the Codex sessions
directory. It joins exactly by the codex thread ids codex-manager returned in the transcript, and
otherwise correlates by originator, worktree and time window; the correlation is conservative, can
miss runs outside this session's window and cannot split cost per workstream. Rank the joined codex
cost against the Claude buckets.

Never drop a codex failure event whose cost does not join (ephemeral, pruned or pre-fix runs).
Surface it and size it with a labeled proxy: the discarded-workstream count times the mean joined
per-workstream cost, plus the orchestrator's own measured reaction tokens. Never present a proxy as
measured.

## Write the retro

Write `retro.md` and `retro.json` to their `retros/<date>-<session>/` directories, then stop at the
gate and have the diagnosis confirmed before applying anything. `retro.md` lists the ranked wastes
with their evidence (the token cost, the `path:line` or memory that held the answer, the axis),
followed by the proposed fixes grouped by destination: ship, plan template, AGENTS.md. `retro.json`
is one object that makes efficacy analysable later:

```
{session_id, name, date, shape: "goal"|"ship", grand_total,
 workstreams: {orchestrator, subagents, codex},
 findings: [{waste_class, axis, cost, cost_kind: "measured"|"proxy",
             fix_destination, fix_id}]}
```

Take the numbers verbatim from `extract.mjs` and never re-estimate them. `waste_class` is a stable
kebab-case slug such as `ungroundable-gate`, `monolithic-self-verify` or `probe-recipe-trial-error`;
reuse the same slug across sessions so a recurrence can be tracked. Give each proposed fix a stable
`fix_id`.

## Apply fixes and log them

On approval, apply the smallest fixes first. Memory writes, sharpened so the next session checks the
fact up front, apply on approval. Edits to ship or the plan template are proposals that change how
every future run behaves, so never apply one without explicit sign-off. The cheapest ship guardrail,
recurring across runs, is to validate each workstream's check command on the clean baseline before
fan-out and reject any gate that is already red.

For every fix you actually apply, append one line to `retros/fixes.jsonl` under the state root. This
is the treatment timeline:

```
{fix_id, waste_class, type: "mechanical-gate"|"judgment", applied_at: <commit SHA>, ref}
```

```
${CLAUDE_PLUGIN_ROOT}/skills/retro/scripts/efficacy.mjs
```

`efficacy.mjs` reads the state root by default, joins the ledger to the `retro.json` records and
reports whether each fix's `waste_class` recurs in later comparable sessions. No recurrence is
near-deductive for a mechanical gate, which makes the waste structurally impossible, and only
suggestive for a judgment fix. The script presents recurrence evidence; a human marks the verdict.
