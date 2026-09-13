#!/usr/bin/env python3
# ABOUTME: Best-effort efficacy analysis for /feature:retro fixes.
# ABOUTME: Joins retro.json outcome records to the fixes ledger and reports waste recurrence per fix.
import argparse, glob, json, os
from collections import defaultdict

DEFAULT_ROOT = os.path.expanduser("~/.claude/retros")


def load_retros(root):
    out = []
    for path in sorted(glob.glob(os.path.join(root, "**", "retro.json"), recursive=True)):
        try:
            out.append(json.load(open(path)))
        except Exception:
            continue
    return out


def load_fixes(root):
    path = os.path.join(root, "fixes.jsonl")
    fixes = []
    if os.path.exists(path):
        for line in open(path):
            line = line.strip()
            if not line:
                continue
            try:
                fixes.append(json.loads(line))
            except Exception:
                continue
    return fixes


def cost_share(retro, waste_class):
    """Sum cost of findings of this class / grand_total, or None if the class is absent."""
    gt = retro.get("grand_total") or 0
    costs = [f.get("cost", 0) for f in retro.get("findings", []) if f.get("waste_class") == waste_class]
    if not costs or not gt:
        return None
    return sum(costs) / gt


def analyze_fix(fix, retros):
    wc = fix.get("waste_class"); applied = fix.get("applied_at") or ""
    shape = fix.get("shape")  # optional: restrict comparison to one session shape
    def eligible(r):
        return shape is None or r.get("shape") == shape
    pre = [r for r in retros if eligible(r) and (r.get("date") or "") < applied]
    post = [r for r in retros if eligible(r) and (r.get("date") or "") >= applied]
    pre_shares = [s for r in pre if (s := cost_share(r, wc)) is not None]
    post_shares = [s for r in post if (s := cost_share(r, wc)) is not None]
    mechanical = fix.get("type") == "mechanical-gate"

    if not post:
        verdict = "INCONCLUSIVE — no comparable post-fix session yet"
    elif not post_shares:
        verdict = ("NO RECURRENCE — strong (mechanical gate: the waste is structurally blocked)"
                   if mechanical else
                   "NO RECURRENCE — weak (judgment/memory fix: absence is not proof)")
    else:
        pre_m = sum(pre_shares) / len(pre_shares) if pre_shares else None
        post_m = sum(post_shares) / len(post_shares)
        if pre_m is not None and post_m < pre_m:
            verdict = f"REDUCED — cost-share {pre_m:.1%} → {post_m:.1%} (recurs smaller; {'gate leak' if mechanical else 'partial'})"
        else:
            base = f"{pre_m:.1%} → " if pre_m is not None else ""
            verdict = f"NOT EFFECTIVE — recurs at {base}{post_m:.1%} in {len(post_shares)} post-fix session(s)"
    return pre, post, pre_shares, post_shares, verdict


def main():
    ap = argparse.ArgumentParser(description="Best-effort efficacy analysis for /feature:retro fixes.")
    ap.add_argument("--root", default=DEFAULT_ROOT, help="retros dir (default ~/.claude/retros)")
    a = ap.parse_args()
    retros = load_retros(a.root)
    fixes = load_fixes(a.root)
    print(f"retros: {len(retros)}  fixes: {len(fixes)}  (root {a.root})")
    if not fixes:
        print("no fixes ledger yet — nothing to score. Apply fixes and append to fixes.jsonl first.")
        return
    by_type = defaultdict(lambda: [0, 0])  # type -> [effective-ish, total]
    for fix in fixes:
        pre, post, pre_s, post_s, verdict = analyze_fix(fix, retros)
        print(f"\n[{fix.get('type')}] {fix.get('fix_id')}  → waste_class={fix.get('waste_class')}  applied={fix.get('applied_at')}")
        print(f"  sessions: {len(pre)} pre / {len(post)} post   "
              f"(class present: {len(pre_s)} pre / {len(post_s)} post)")
        print(f"  VERDICT: {verdict}")
        t = by_type[fix.get("type")]
        t[1] += 1
        if verdict.startswith(("NO RECURRENCE", "REDUCED")):
            t[0] += 1
    print("\nby type (recurrence-free or reduced / total):")
    for k, (good, tot) in sorted(by_type.items()):
        print(f"  {k}: {good}/{tot}")
    print("\nThis is recurrence evidence, not proof. Mark each fix effective/ineffective/inconclusive "
          "by judgment; only mechanical-gate NO-RECURRENCE is near-deductive.")


if __name__ == "__main__":
    main()
