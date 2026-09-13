#!/usr/bin/env python3
# ABOUTME: Objective retro evidence for a named Claude Code session — spawn ledger + token-share-by-role.
# ABOUTME: Joins orchestrator Task spawns to subagents/<agent>.meta.json (toolUseId) and sums each child's tokens.
import argparse, glob, json, os, re, subprocess
from collections import Counter, defaultdict
from datetime import datetime

DEFAULT_ROOT = os.path.expanduser("~/.claude/projects")
DEFAULT_CODEX_ROOT = os.path.expanduser("~/.codex/sessions")


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


def _parse_ts(value):
    try:
        return datetime.fromisoformat(str(value).replace("Z", "+00:00")).timestamp()
    except Exception:
        return None


def orch_context(orch):
    """cwd, [min_ts, max_ts], and any codex thread ids recorded in the orchestrator transcript."""
    cwds = Counter(); lo = hi = None; threads = set()
    for line in open(orch):
        try:
            o = json.loads(line)
        except Exception:
            continue
        if o.get("cwd"):
            cwds[o["cwd"]] += 1
        ts = _parse_ts(o.get("timestamp"))
        if ts is not None:
            lo = ts if lo is None else min(lo, ts)
            hi = ts if hi is None else max(hi, ts)
        # "threads": ["id", ...] recorded by codex:implement's status JSON — it arrives
        # inside a tool_result string, so unescape one level of JSON quoting first.
        unescaped = line.replace('\\"', '"')
        for m in re.finditer(r'"threads"\s*:\s*\[([^\]]*)\]', unescaped):
            threads.update(re.findall(r'"([^"]+)"', m.group(1)))
    cwd = cwds.most_common(1)[0][0] if cwds else None
    return cwd, lo, hi, threads


def _codex_meta(path):
    with open(path) as fh:
        for _ in range(5):
            line = fh.readline()
            if not line:
                break
            try:
                o = json.loads(line)
            except Exception:
                continue
            if o.get("type") == "session_meta":
                return o.get("payload", o)
    return None


def _codex_tokens(path, basis):
    """Last cumulative thread_token_usage in the rollout, on the requested basis."""
    u = {}
    for line in open(path):
        if "token_usage_record" not in line:
            continue
        try:
            o = json.loads(line)
        except Exception:
            continue
        tt = (o.get("payload", o) or {}).get("thread_token_usage")
        if isinstance(tt, dict) and tt.get("total_tokens") is not None:
            u = tt
    i = u.get("input_tokens", 0); cw = u.get("cache_write_input_tokens", 0)
    out = u.get("output_tokens", 0); rsn = u.get("reasoning_output_tokens", 0)
    return {"output": out + rsn, "billable": i + cw + out + rsn, "full": u.get("total_tokens", 0)}[basis]


def codex_cost(orch, codex_root, basis):
    """Join the codex lane's real token cost: by recorded thread id (deterministic),
    else by originator + cwd + time window (correlation). Returns (rows, deterministic_bool)."""
    cwd, lo, hi, threads = orch_context(orch)
    rows = []
    # recursive ** matches rollouts in date subdirs and directly under codex_root
    for path in sorted(set(glob.glob(os.path.join(codex_root, "**", "rollout-*.jsonl"), recursive=True))):
        meta = _codex_meta(path)
        if not meta:
            continue
        sid = meta.get("id") or meta.get("session_id")
        det = sid in threads
        ts = _parse_ts(meta.get("timestamp"))
        corr = (meta.get("originator") == "Claude Code" and cwd and meta.get("cwd") == cwd
                and ts is not None and lo is not None and hi is not None and lo <= ts <= hi)
        if det or corr:
            rows.append((_codex_tokens(path, basis), sid, "exact" if det else "corr", os.path.basename(path)))
    rows.sort(reverse=True)
    return rows, bool(threads)


def report(name, root, basis, codex_root):
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
        rows, deterministic = codex_cost(orch, codex_root, basis)
        join = "exact thread-id join" if deterministic else "correlation (originator+cwd+time) — heuristic"
        print(f"  --- codex lane ({len(rows)} rollouts, {basis} tokens; {join}) ---")
        ctot = 0
        for tk, sid, how, name in rows:
            print(f"    {tk:>12,}  [{how}] {str(sid)[:36]}  {name[:44]}")
            ctot += tk
        if rows:
            print(f"  codex TOTAL: {ctot:,}  (NOT in the Claude transcripts; joined from ~/.codex/sessions)")
            print("  ** rank this against the Claude buckets above — a failed/nothing-merged codex run "
                  "belongs in the ranking, never dropped because its cost lived off-transcript. **")
        else:
            print("  ** codex lane present but no rollouts joined (ephemeral runs, pruned, or no match). "
                  "Rank its failure events by impact: discarded-workstream count x mean joined per-workstream "
                  "cost, plus the orchestrator's own reaction tokens. Never drop the event. **")
    print(f"  basis={basis} excludes cache_read (Claude) / cached_input (codex) unless 'full'.")


def main():
    ap = argparse.ArgumentParser(description="Objective retro evidence for a named Claude Code session.")
    ap.add_argument("names", nargs="+", help="session title(s), as set by /rename")
    ap.add_argument("--root", default=DEFAULT_ROOT, help="projects dir (default ~/.claude/projects)")
    ap.add_argument("--basis", default="billable", choices=["output", "billable", "full"],
                    help="token counting basis (default billable = input+cache_creation+output)")
    ap.add_argument("--codex-root", default=DEFAULT_CODEX_ROOT,
                    help="codex sessions dir (default ~/.codex/sessions)")
    a = ap.parse_args()
    for n in a.names:
        report(n, a.root, a.basis, a.codex_root)


if __name__ == "__main__":
    main()
