#!/usr/bin/env python3
# ABOUTME: Checks that every `path:line` anchor in a grounding doc points at the symbol its sentence names.
# ABOUTME: Usage: check-anchors.py [--at <sha>] <doc> [skip-regex] — run inside the repo; exits 1 on any flag.
#
# Lines are read at the doc's `Base: `<sha>`` commit (or --at, else the working tree). For each
# anchor, an identifier from a backticked span or a "quoted phrase" in the same sentence must
# occur within WINDOW lines of the cited range; a sentence that names nothing is NOSYMBOL. A
# backticked `name()` call must be cited by some anchor of its paragraph, else it is UNANCHORED.
# A bare `:N` anchor continues the last file named; a list item is its own paragraph. Fenced
# code blocks are output, not claims.
import re
import subprocess
import sys

WINDOW = 3
SHOW = 6  # candidates listed in a flag line

LINES = r'\d+(?:-(?:\d+|…))?(?:,\d+(?:-\d+)?)*'
ANCHOR = re.compile(rf'([^`\s:]+):({LINES})')
CONTINUATION = re.compile(rf':({LINES})')
PATH = re.compile(r'[\w@./\[\]-]*\.(ts|tsx|js|mjs|sql|json|jsonc|sh|toml|ya?ml|md|txt|env)')
SPAN = re.compile(r'`([^`]+)`')
QUOTED = re.compile(r'"([^"`]{3,})"')
CALL = re.compile(r'^([A-Za-z_$][\w$.]*)\(')
IDENT = re.compile(r'[A-Za-z_$][\w$]{2,}')
BASE = re.compile(r'^Base: `([0-9a-f]{6,40})`', re.M)
SENTENCE_END = re.compile(r'(?<=[.;])\s+')


def usage():
    print('usage: check-anchors.py [--at <sha>] <doc> [skip-regex]', file=sys.stderr)
    sys.exit(2)


def git(*args):
    return subprocess.run(['git', *args], capture_output=True, text=True)


class Source:
    """File contents at one commit, or the working tree when sha is None."""

    def __init__(self, root, sha):
        self.root, self.sha, self.files, self.tree = root, sha, {}, None

    def names(self):
        if self.tree is None:
            cmd = ['ls-tree', '-r', '--name-only', self.sha] if self.sha else ['ls-files']
            self.tree = git(*cmd).stdout.split('\n')
        return self.tree

    def lines(self, path):
        if path not in self.files:
            text = None
            if self.sha:
                r = git('show', f'{self.sha}:{path}')
                text = r.stdout if r.returncode == 0 else None
            else:
                try:
                    with open(f'{self.root}/{path}', encoding='utf-8', errors='replace') as f:
                        text = f.read()
                except OSError:
                    pass
            self.files[path] = None if text is None else text.rstrip('\n').split('\n')
        return self.files[path]

    def resolve(self, path):
        if '/' in path:
            return path, None
        hits = [n for n in self.names() if n == path or n.endswith('/' + path)]
        if len(hits) == 1:
            return hits[0], None
        return None, 'MISSING' if not hits else f'AMBIGUOUS ({len(hits)} matches)'


def strip_fences(text):
    return re.sub(r'^```.*?^```[ \t]*$', '', text, flags=re.M | re.S)


def paragraphs(text):
    """Blank-line-separated blocks, with every list item as its own paragraph."""
    return [p for p in re.split(r'\n\s*\n|\n(?=\s*(?:[-*+]|\d+\.)\s)', text) if p.strip()]


def sentences(paragraph):
    """Split at sentence ends outside backticks; each sentence is a list of (is_span, text)."""
    out, cur = [], []
    for i, piece in enumerate(re.split(r'(`[^`]+`)', paragraph)):
        if i % 2:
            cur.append((True, piece[1:-1]))
            continue
        parts = SENTENCE_END.split(piece)
        for j, part in enumerate(parts):
            if j:
                out.append(cur)
                cur = []
            cur.append((False, part))
    out.append(cur)
    return out


def ranges(spec):
    for part in spec.split(','):
        lo, _, hi = part.partition('-')
        yield int(lo), int(lo if hi in ('', '…') else hi)


def candidates(sentence):
    """Search strings the sentence names: identifiers from backticked spans, quoted phrases."""
    found = []
    for is_span, text in sentence:
        if is_span:
            if ANCHOR.fullmatch(text) or CONTINUATION.fullmatch(text) or PATH.fullmatch(text):
                continue
            found.append(text.strip())
            found += IDENT.findall(text)
        else:
            found += QUOTED.findall(text)
    return list(dict.fromkeys(s for s in found if s))


def calls(paragraph):
    return [m.group(1) for span in SPAN.findall(paragraph) for m in [CALL.match(span)] if m]


def anchors(sentence, last_path):
    """(path, spec) per anchor in the sentence; bare paths and `:N` continue the last path."""
    out = []
    for is_span, text in sentence:
        if not is_span:
            continue
        a, c = ANCHOR.fullmatch(text), CONTINUATION.fullmatch(text)
        if a:
            last_path, spec = a.group(1), a.group(2)
        elif c and last_path:
            spec = c.group(1)
        elif PATH.fullmatch(text):
            last_path = text
            continue
        else:
            continue
        out.append((last_path, spec))
    return out, last_path


def window(lines, lo, hi):
    return lines[max(lo - 1 - WINDOW, 0):min(hi + WINDOW, len(lines))], max(lo - WINDOW, 1)


def found_in(name, body):
    tail = name.rsplit('.', 1)[-1]
    return any(name in line or tail in line for line in body)


def main(argv):
    sha = None
    if argv[:1] == ['--at']:
        if len(argv) < 2:
            usage()
        sha, argv = argv[1], argv[2:]
    if not argv:
        usage()
    doc, skip = argv[0], (re.compile(argv[1]) if len(argv) > 1 else None)
    r = git('rev-parse', '--show-toplevel')
    if r.returncode != 0:
        print('check-anchors.py: not inside a git repo', file=sys.stderr)
        return 2
    with open(doc, encoding='utf-8') as f:
        text = f.read()
    if sha is None:
        m = BASE.search(text)
        sha = m.group(1) if m else None
    src = Source(r.stdout.strip(), sha)
    at = f'at {sha}' if sha else 'in the working tree'
    flagged = False
    last_path = None

    for para in paragraphs(strip_fences(text)):
        cited = []  # windows this paragraph cites, for the UNANCHORED check
        for sentence in sentences(para):
            found, last_path = anchors(sentence, last_path)
            wanted = candidates(sentence)
            for path, spec in found:
                if skip and skip.search(path):
                    continue
                real, err = src.resolve(path)
                lines = None if err else src.lines(real)
                if err and err != 'MISSING':
                    print(f'{err.split()[0]}: {path} {err.split(" ", 1)[1]}')
                elif lines is None:
                    print(f'MISSING: {path} ({at})')
                else:
                    for lo, hi in ranges(spec):
                        if hi > len(lines):
                            print(f'PAST-EOF: {path}:{lo if lo == hi else f"{lo}-{hi}"} (file has {len(lines)} lines {at})')
                            flagged = True
                            continue
                        body, start = window(lines, lo, hi)
                        cited.append(body)
                        if not wanted:
                            print(f'NOSYMBOL: {path}:{spec} — the sentence names nothing to look for')
                        elif not any(s in line for s in wanted for line in body):
                            shown = wanted[:SHOW] + (['…'] if len(wanted) > SHOW else [])
                            print(f'UNVERIFIED: {path}:{spec} — none of [{", ".join(shown)}] within {WINDOW} lines')
                        else:
                            continue
                        flagged = True
                        for i, line in enumerate(body):
                            print(f'    {start + i}| {line}')
                    continue
                flagged = True
        for name in dict.fromkeys(calls(para)):
            if not any(found_in(name, body) for body in cited):
                print(f'UNANCHORED: {name}() — no path:line anchor in its paragraph cites it')
                flagged = True
    return 1 if flagged else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
