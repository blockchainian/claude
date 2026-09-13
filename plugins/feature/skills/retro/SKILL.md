---
name: retro
description: >
  Find the biggest wastes of effort in a finished orchestration session and
  route each fix back into the workflow it came from. Run in a SEPARATE
  session, passing the finished session's name: "/feature:retro <session-name>".
  Reads that session's transcript as evidence, ranks wastes by real token cost,
  and proposes fixes to ground, the planner and ship. NOT in the session being
  analysed (a session grading itself inherits the blind spot that caused the
  waste), and not for a one-turn task with nothing to rank.
---

# Retro — find the waste, route the fix

A session that wasted effort did so because of a flawed model of the world —
it did not think to check the gate, or to read the existing client. Asking that
same warm context to critique itself inherits the exact blind spot, and its
self-estimates are unreliable: a real run reported "~2.65M subagent tokens, 87%
Fable" when the transcript held 11.24M, 84% — the ratio roughly right, the
magnitude 4x wrong and matching no real accounting basis. So this runs in a
**fresh session** that reads the finished session's transcript as evidence: an
auditor, not the author. The transcript's own stated reasoning is available, but
treated as a claim to verify, never as justification for a deviation.

The output is one file, `~/.claude/retros/<date>-<session>/retro.md`: the ranked
wastes with their evidence, then the proposed fixes. Write it there, never into a
repo — the analysed session's worktree is often deleted after it finishes, and
this stable archive sits beside the transcripts it reads (`~/.claude/projects/`).
Diagnosis and fixes are two gates — write and confirm the diagnosis before
touching anything.

## Procedure

1. **Extract the objective evidence.** The join is deterministic and needs no
   instrumentation — a spawn's `tool_use.id` matches `subagents/<agent>.meta.json`
   `toolUseId`, and each child transcript carries its own token usage:

   ```
   ${CLAUDE_PLUGIN_ROOT}/skills/retro/extract.py <session-name>
   ```

   It prints the orchestrator's own cost, the spawn ledger (count by
   `subagent_type`), and every subagent ranked by billable tokens with its
   model, agent type and workstream label. It also prints the orchestrator
   transcript path and `subagents/` dir — grep those for the rest of step 2.

2. **Trace the expensive turns to their origin.** Token cost points at *where*
   effort went; the transcript says *why*. For each costly cluster — a re-run
   workstream, a repeated probe, a long tail — grep the orchestrator transcript
   for the gate result, the error, the re-do. The one rule that decides
   attribution:

   > A failed gate is not proof of bad code. Ask whether the **code** was wrong
   > or the **plan/gate** was wrong. A run where every workstream "failed its
   > check" but all wrote correct code is a planner defect, not an agent defect.

3. **Classify each waste on one axis, and name where the answer already was.**
   Every finding carries a token cost and a pointer to the fact/rule that would
   have prevented it; drop any finding that has neither.

   - **Axis A — knowable-fact miss.** The answer already existed in memory or the
     repo (a memory that warned of the gate; an existing client that held the
     auth recipe). Fix feeds **`/feature:ground`**.
   - **Axis B — workflow/topology deviation.** The session did not run the shape
     ship prescribes — UX work as `general-purpose` agents instead of
     `ux-implementer` + `ux-verifier`, hand-driving instead of probe-first. The
     spawn ledger shows this directly. Classify each as **lapse** (ship was
     clear, discipline failed → a ship guardrail) or **signal** (the shape did
     not fit → change the workflow). Fix feeds **`/feature:ship`**.
   - **Axis C — planner defect.** The plan encoded a gate/spec/scope/criterion
     that could not hold and detonated only downstream: a check that can't pass
     on baseline, a value contradicting existing code, an under-scoped workstream
     whose gate was too narrow to catch cross-file breakage, an unsatisfiable
     success criterion, or missing de-risk sequencing (no pilot, no probe-first).
     Fix feeds the **`planner`** agent + `skills/ship/plan-template.md`.

   Keep an **Inherent (not waste)** bucket: test retargeting, legitimate
   exploration, a gate-fix the plan could not have avoided. Honesty about what
   was not waste is what makes the ranking credible.

4. **Account for the codex lane.** Codex runs in an external runtime, so its cost
   is not in the Claude transcript — but `extract.py` recovers it from
   `~/.codex/sessions`: by the codex thread id that `codex:implement` now records
   in its status JSON (exact), else by originator + worktree + time window
   (correlation — conservative, may miss runs outside this session's window and
   cannot split per-workstream). Rank the joined codex cost against the Claude
   buckets. For a codex **failure event** whose cost does not join (ephemeral,
   pruned, pre-fix), never drop it: surface it and size it by a labeled proxy —
   discarded-workstream count × mean joined per-workstream cost, plus the
   orchestrator's own (measured) reaction tokens. Never present a proxy as measured.

5. **Write `retro.md` to `~/.claude/retros/<date>-<session>/` and stop at the
   gate.** Ranked wastes with evidence (token cost, the `path:line` or memory that
   held the answer, the axis). Then the proposed fixes, grouped by destination
   (ground / planner / ship / memory). Confirm the diagnosis before applying anything.

6. **Apply, on approval, smallest first.** Memory writes (sharpened so the next
   session front-loads the check) apply on approval. Edits to `ground`, the
   `planner` agent, `ship` or the plan template are **proposals** — they change
   how every future run behaves, so never apply one without explicit sign-off.
   The cheapest ship guardrail, recurring across runs: validate each workstream's
   check command on the clean baseline before fan-out, and reject any gate
   already red.

## Scaling

Run it after an orchestration session big enough to have a spawn ledger worth
ranking — a multi-workstream ship, a sweep. Skip it for a session that spawned
nothing: there is no waste to rank, and its lessons belong in a plain memory.
Verifying "the answer already existed" (Axis A) is a read-only repo/memory
check — delegate that fan-out to Explore agents (`model: "sonnet"`).
