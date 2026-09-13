#!/usr/bin/env python3
# ABOUTME: Checks that every acceptance criterion in a problem.md is turned into a referenced test in its plan.md.
# ABOUTME: Usage: check-acceptance.py <problem.md>  |  check-acceptance.py <plan.md> <problem.md> — exits 1 on any flag.
#
# One arg (a problem.md): the "## Accepted when" section must hold at least one criterion,
# each a bullet naming a unique id `AC<n>`. Empty or "none" is NO-ACCEPTANCE; a repeated id
# is DUPLICATE. Two args (a plan.md and its problem.md): every criterion id must be referenced
# somewhere in the plan (typically as `[AC<n>]` on a Tests: line or a UX-checklist assertion);
# a criterion no test references is UNCOVERED, a plan reference to an undefined id is UNKNOWN.
import re
import sys

CRITERION = re.compile(r'^\s*[-*]\s*\**\s*(AC\d+)\b')
REFERENCE = re.compile(r'\bAC\d+\b')
SECTION = re.compile(r'^##+\s+Accepted when\s*$', re.I)
HEADING = re.compile(r'^##\s')


def usage():
    print('usage: check-acceptance.py <problem.md> | check-acceptance.py <plan.md> <problem.md>', file=sys.stderr)
    sys.exit(2)


def read(path):
    try:
        with open(path, encoding='utf-8') as f:
            return f.read()
    except OSError as e:
        print(f'check-acceptance.py: cannot read {path}: {e.strerror}', file=sys.stderr)
        sys.exit(2)


def acceptance_section(text):
    """The lines under '## Accepted when', up to the next '## ' heading."""
    lines = text.splitlines()
    out = []
    inside = False
    for line in lines:
        if SECTION.match(line):
            inside = True
            continue
        if inside and HEADING.match(line):
            break
        if inside:
            out.append(line)
    return out


def criteria(text, flags):
    """Ordered criterion ids defined in the acceptance section; records DUPLICATE flags."""
    ids = []
    seen = set()
    for line in acceptance_section(text):
        m = CRITERION.match(line)
        if not m:
            continue
        cid = m.group(1)
        if cid in seen:
            flags.append(('DUPLICATE', cid))
        else:
            seen.add(cid)
            ids.append(cid)
    return ids


def num(cid):
    return int(cid[2:])


def main():
    args = sys.argv[1:]
    if len(args) == 1:
        plan, problem = None, args[0]
    elif len(args) == 2:
        plan, problem = args
    else:
        usage()
        return 2

    flags = []
    defined = criteria(read(problem), flags)
    if not defined:
        flags.append(('NO-ACCEPTANCE', None))

    if plan is not None and defined:
        referenced = set(REFERENCE.findall(read(plan)))
        for cid in defined:
            if cid not in referenced:
                flags.append(('UNCOVERED', cid))
        for cid in sorted(referenced - set(defined), key=num):
            flags.append(('UNKNOWN', cid))

    if not flags:
        return 0

    order = {'NO-ACCEPTANCE': 0, 'DUPLICATE': 1, 'UNCOVERED': 2, 'UNKNOWN': 3}
    for kind, cid in sorted(flags, key=lambda f: (order[f[0]], num(f[1]) if f[1] else 0)):
        if kind == 'NO-ACCEPTANCE':
            print('NO-ACCEPTANCE: no acceptance criteria found')
        else:
            print(f'{kind}: {cid}')
    return 1


if __name__ == '__main__':
    sys.exit(main())
