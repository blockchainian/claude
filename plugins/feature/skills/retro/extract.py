#!/usr/bin/env python3
# ABOUTME: Objective retro evidence for a named Claude Code session — spawn ledger + token-share-by-role.
# ABOUTME: Joins orchestrator Task spawns to subagents/<agent>.meta.json (toolUseId) and sums each child's tokens.
import argparse, glob, json, os, subprocess
from collections import Counter, defaultdict

DEFAULT_ROOT = os.path.expanduser("~/.claude/projects")


def resolve(name, root):
    """Session name -> its top-level transcript path(s), via the customTitle record /rename writes."""
    out = subprocess.run(
        ["grep", "-rl", f'"customTitle":"{name}"', "--include=*.jsonl", root],
        capture_output=True, text=True).stdout.split()
    return [p for p in out if "/subagents/" not in p]


def _tok(u, basis):
    i = u.get("input_tokens", 0); cc = u.get("cache_creation_input_tokens", 0)
    o = u.get("output_tokens", 0); cr = u.get("cache_read_input_tokens", 0)
    return {"output": o, "billable": i + cc + o, "full": i + cc + o + cr}[basis]


def file_stats(path, basis):
    """Return (tokens, turns, primary_model) for one transcript, model taken from its own turns."""
    tokens = turns = 0
    models = Counter()
    for line in open(path):
        try:
            o = json.loads(line)
        except Exception:
            continue
        if o.get("type") == "assistant":
            m = o.get("message", {})
            models[m.get("model")] += 1
            turns += 1
            tokens += _tok(m.get("usage", {}), basis)
    primary = models.most_common(1)[0][0] if models else "?"
    return tokens, turns, primary


def spawns(orch):
    """tool_use id -> {subagent_type, desc} for every Task/Agent spawn in the orchestrator transcript."""
    out = {}
    for line in open(orch):
        try:
            o = json.loads(line)
        except Exception:
            continue
        m = o.get("message", {})
        content = m.get("content") if isinstance(m, dict) else None
        if isinstance(content, list):
            for b in content:
                if isinstance(b, dict) and b.get("type") == "tool_use" and b.get("name") in ("Task", "Agent"):
                    inp = b.get("input", {})
                    out[b.get("id")] = {"subagent_type": inp.get("subagent_type"), "desc": inp.get("description")}
    return out


def has_codex_lane(sp):
    return any((v["subagent_type"] or "").startswith("codex:") for v in sp.values())


def report(name, root, basis):
    print(f"\n==== {name} ====")
    paths = resolve(name, root)
    if not paths:
        print(f"  !! no session titled {name!r} under {root}")
        return
    orch = paths[0]
    print(f"  orchestrator transcript: {orch}")
    if len(paths) > 1:
        print(f"  (note: {len(paths)} transcripts carry this title; using the first — grep the rest by hand)")

    ot, oturns, omodel = file_stats(orch, basis)
    print(f"  orchestrator own cost: {ot:,} tok ({basis}), {oturns} turns, {omodel}")

    sp = spawns(orch)
    ledger = Counter(v["subagent_type"] for v in sp.values())
    print(f"  spawn ledger: {dict(ledger)}")

    sdir = orch[:-len(".jsonl")] + "/subagents"
    print(f"  subagents dir: {sdir}")
    if not os.path.isdir(sdir):
        print("  NO subagents/ dir — no Claude subagent cost recorded for this session.")
    else:
        rows = []
        by_model = defaultdict(int); total = 0
        for meta_path in glob.glob(sdir + "/*.meta.json"):
            try:
                meta = json.load(open(meta_path))
            except Exception:
                continue
            jpath = meta_path[:-len(".meta.json")] + ".jsonl"
            if not os.path.exists(jpath):
                continue
            tk, turns, model = file_stats(jpath, basis)
            label = (sp.get(meta.get("toolUseId"), {}) or {}).get("desc") or meta.get("description") or "?"
            rows.append((tk, model, meta.get("agentType"), turns, label))
            by_model[model] += tk; total += tk
        rows.sort(reverse=True)
        print(f"  --- subagents ({len(rows)}), ranked by {basis} tokens ---")
        for tk, model, atype, turns, label in rows:
            print(f"    {tk:>12,}  {str(model):<18} {str(atype):<24} turns={turns:<4} {str(label)[:52]}")
        if total:
            share = ", ".join(f"{k}={v:,} ({v / total * 100:.0f}%)"
                              for k, v in sorted(by_model.items(), key=lambda x: -x[1]))
            print(f"  subagent TOTAL: {total:,}  |  {share}")
            print(f"  session grand total (orchestrator + subagents): {ot + total:,}")

    if has_codex_lane(sp):
        print("  ** codex lane present: its implement/rescue token cost is NOT in these transcripts "
              "(external runtime). The plan and that rescue ran are visible; the codex burn is not. **")
    print(f"  basis={basis} excludes cache_read unless 'full'. cache_read is large but near-free.")


def main():
    ap = argparse.ArgumentParser(description="Objective retro evidence for a named Claude Code session.")
    ap.add_argument("names", nargs="+", help="session title(s), as set by /rename")
    ap.add_argument("--root", default=DEFAULT_ROOT, help="projects dir (default ~/.claude/projects)")
    ap.add_argument("--basis", default="billable", choices=["output", "billable", "full"],
                    help="token counting basis (default billable = input+cache_creation+output)")
    a = ap.parse_args()
    for n in a.names:
        report(n, a.root, a.basis)


if __name__ == "__main__":
    main()
