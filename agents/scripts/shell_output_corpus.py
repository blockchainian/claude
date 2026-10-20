#!/usr/bin/env python3
"""Generate public shell-output ground truth and run the private Rust corpus audit.

Generate: python3 scripts/shell_output_corpus.py generate
Audit:    python3 scripts/shell_output_corpus.py check --calls CALLS --replay REPLAY --report REPORT

Each JSONL record contains provider, command, stdout, error, exit and parts.
Generated records also contain expected, a list of {start, end, language} spans
with zero-based half-open line ranges. Omitted lines are plain text. Ground truth
comes from separately executing each command stage, never from the segmenter.
Only repository files named below are read by generate; private inputs stay local.
"""

import argparse
import hashlib
import json
import os
import re
import shlex
from pathlib import Path
import subprocess
import sys
import time

AGENTS = Path(__file__).resolve().parents[1]
ROOT = AGENTS.parent
DEFAULT_FIXTURE = AGENTS / 'tests' / 'fixtures' / 'shell-output-corpus.jsonl'

# Command shapes are taken from recorded shell calls. Paths and ranges are
# replaced with this public repository's own files. A stage is (command, language).
# Plain output has language None, including numbered search results and summaries.
CASES = [
    [('cd agents', None), ('sed -n 35,70p ../tests/marketplace.test.mjs', 'mjs')],
    [('sed -n 1,10000p agents/Cargo.toml', 'toml')],
    [('head -n 5 agents/src/main.rs', 'rs')],
    [('cat agents/Cargo.toml', 'toml')],
    [('cat agents/AGENTS.md AGENTS.md', 'md')],
    [('nl -ba agents/Cargo.toml', 'toml')],
    [('sed -n 1,5p agents/src/main.rs', 'rs'), ('sed -n 6,10p agents/src/main.rs', 'rs')],
    [('sed -n 1,5p agents/src/main.rs', 'rs'), ('head -n 5 agents/themes/build.py', 'py')],
    [('echo boundary', None), ('head -n 5 agents/src/main.rs', 'rs')],
    [('head -n 5 agents/src/main.rs', 'rs'), ('echo boundary', None)],
    [('head -n 5 agents/src/main.rs', 'rs'), ('echo boundary', None), ('head -n 5 agents/themes/build.py', 'py')],
    [('echo', None), ('head -n 5 agents/src/main.rs', 'rs'), ('echo', None)],
    [('head -n 5 agents/src/main.rs', 'rs'), ('wc -l agents/Cargo.toml', None)],
    [('head -n 5 agents/src/main.rs', 'rs'), ('grep -n package agents/Cargo.toml', None)],
    [('grep -n package agents/Cargo.toml', None), ('head -n 5 agents/src/main.rs', 'rs')],
    [('head -n 5 agents/src/main.rs', 'rs'), ('ls agents/src/providers', None)],
    [('head -n 5 agents/src/main.rs', 'rs'), ('rg -n package agents/Cargo.toml', None)],
    [('git status --short -- agents/AGENTS.md', None), ('head -n 5 agents/src/main.rs', 'rs')],
    [('git log --oneline -1 -- agents/AGENTS.md', None), ('head -n 5 agents/src/main.rs', 'rs')],
    [('cat agents/Cargo.toml | head -n 5', 'toml')],
    [('cat agents/Cargo.toml | sed -n 2,5p', 'toml')],
    [('git diff --no-index -- agents/AGENTS.md AGENTS.md', 'diff')],
    [('git diff --no-index --stat -- agents/AGENTS.md AGENTS.md', None)],
    [('head -n 5 agents/src/main.rs', 'rs'), ('grep -n certainly_not_a_repository_symbol agents/Cargo.toml', None)],
    [('head -n 5 agents/src/main.rs', 'rs'), ('head -n 5 agents/not-a-file.rs', None)],
]


# A short read must stay colored beside line-formatted output after slicing.
PIPELINE_CASES = [
    stages
    for producer in ('rg -n package agents/Cargo.toml',
                     'grep -n package agents/Cargo.toml',
                     'git log --oneline -1 -- agents/AGENTS.md')
    for filter_command in ('head -25', 'tail -n 25', 'sed -n 1,25p',
                           'head -25 | tail -n 25 | cat')
    for stages in (
        [('sed -n 1,10000p agents/Cargo.toml', 'toml'),
         (f'{producer} | {filter_command}', None)],
        [(f'{producer} | {filter_command}', None),
         ('sed -n 1,10000p agents/Cargo.toml', 'toml')],
    )
]


def execute_case(stages, provider):
    """Run stages separately, preserving cd state and && short-circuiting."""
    cwd = ROOT
    parts, exit_code, stderr = [], 0, ''
    languages = []
    for command, language in stages:
        if exit_code:
            parts.append('')
            languages.append(None)
            continue
        if command.startswith('cd '):
            cwd = (cwd / command[3:]).resolve()
            parts.append('')
            languages.append(None)
            continue
        process = subprocess.run(command, shell=True, executable='/bin/sh', cwd=cwd,
                                 text=True, stdout=subprocess.PIPE,
                                 stderr=subprocess.STDOUT if provider == 'codex' else subprocess.PIPE,
                                 timeout=10, check=False)
        exit_code = process.returncode
        parts.append(process.stdout)
        languages.append(language)
        if provider == 'claude':
            stderr += process.stderr
    joined = ''.join(parts)
    stdout = joined.strip('\n') if provider == 'claude' else joined
    lead = len(joined) - len(joined.lstrip('\n')) if provider == 'claude' else 0
    expected, offset = [], 0
    total = len(stdout.splitlines())
    for part, language in zip(parts, languages):
        count = len(part.splitlines())
        start, end = max(0, offset - lead), min(total, offset + count - lead)
        if language and start < end:
            expected.append({'start': start, 'end': end, 'language': language})
        offset += count
    return {'provider': provider, 'command': ' && '.join(c for c, _ in stages),
            'stdout': stdout, 'error': exit_code != 0, 'exit': exit_code,
            'parts': parts, 'expected': expected}


def generate(args):
    records = [execute_case(stages, provider)
               for provider in ('claude', 'codex') for stages in CASES]
    for provider in ('claude', 'codex'):
        for stages in PIPELINE_CASES:
            record = execute_case(stages, provider)
            record['minimum_highlighted'] = sum(s['end'] - s['start'] for s in record['expected'])
            records.append(record)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('w') as output:
        for record in records:
            output.write(json.dumps(record, ensure_ascii=False, separators=(',', ':')) + '\n')
    print(json.dumps({'records': len(records), 'output': str(args.output)}))


def segments(command):
    """Split simple shell commands independently from the Rust implementation."""
    lex = shlex.shlex(command, posix=True, punctuation_chars=';&|')
    lex.whitespace = ' \t\r'
    lex.commenters = ''
    lex.wordchars += ':'
    result, words = [], []
    try:
        for token in lex:
            if token in (';', '&&', '||', '\n', '&'):
                if words:
                    result.append(words)
                    words = []
            else:
                words.append(token)
    except ValueError:
        return None
    if words:
        result.append(words)
    return result


ALIASES = {'mjs': 'js', 'cjs': 'js', 'jsx': 'js', 'yml': 'yaml',
           'bash': 'sh', 'zsh': 'sh', 'jsonc': 'json', 'cc': 'cpp', 'hpp': 'cpp'}
LANGUAGES = set('rs py js mjs cjs ts tsx jsx json jsonc toml yaml yml sh bash zsh md swift go c h cc cpp hpp java kt rb css scss html xml sql lua'.split())

# Additional replay file types accepted by the dashboard syntax set.
SCOPE_LANGUAGES = LANGUAGES | {'log', 'txt', 'tsv'}


def language_name(language, expanded=True):
    if language is None:
        return None
    language = language.removeprefix('numbered:')
    # Reference scoring retains the prototype language set. Expanded scoring
    # includes the additional file syntaxes present in the replay corpus.
    return ALIASES.get(language, language) if language in (SCOPE_LANGUAGES if expanded else LANGUAGES) or language == 'diff' else None


def file_language(path):
    suffix = Path(path).suffix.lstrip('.').lower()
    return suffix if suffix in SCOPE_LANGUAGES else None


def stage_language(words):
    """Ground truth classifies each actual stage, never guesses its output length."""
    while words and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*=.*', words[0]):
        words = words[1:]
    if not words:
        return None
    if words[0] == 'cat':
        if any(w.startswith('-') and w != '-n' for w in words[1:]):
            return None
        paths = [w for w in words[1:] if not w.startswith('-')]
    elif words[0] in ('head', 'tail'):
        options = words[1:-1]
        if not (options == [] or
                len(options) == 1 and re.fullmatch(r'-n?\d+', options[0]) or
                len(options) == 2 and options[0] == '-n' and re.fullmatch(r'\+?\d+', options[1])):
            return None
        paths = words[-1:]
    elif words[0] == 'nl' and len(words) == 3 and words[1] == '-ba':
        paths = words[-1:]
    elif (words[0] == 'sed' and len(words) == 4 and words[1] == '-n'
          and re.fullmatch(r'\d+(?:,(?:\d+|\$))?p', words[2])):
        paths = words[-1:]
    elif len(words) == 3 and words[:2] == ['git', 'show'] and ':' in words[2]:
        return file_language(words[2].split(':', 1)[1])
    elif words[:2] in (['git', 'diff'], ['git', 'show'], ['git', 'log']):
        summary = ('--stat', '--name-only', '--name-status', '--shortstat', '--numstat',
                   '--quiet', '--check', '-s', '--no-patch', '--oneline', '--summary', '--dirstat')
        if any(w in summary or w.startswith(('--format', '--pretty', '--stat')) or ':' in w
               for w in words[2:]):
            return None
        return 'diff' if words[1] != 'log' or any(w in ('-p', '--patch') for w in words[2:]) else None
    else:
        return None
    languages = {language_name(file_language(p)) for p in paths}
    return next(iter(languages)) if len(languages) == 1 else None


def pipeline_language(words):
    stages, stage = [], []
    for word in words:
        if word == '|':
            stages.append(stage)
            stage = []
        else:
            stage.append(word)
    stages.append(stage)
    language = stage_language(stages[0])
    for stage in stages[1:]:
        if not stage or stage[0] not in ('cat', 'head', 'tail', 'sed'):
            return None
    return language


def truth(record):
    rows = record['stdout'].splitlines()
    expected = [None] * len(rows)
    if 'expected' in record:
        for span in record['expected']:
            for index in range(span['start'], span['end']):
                expected[index] = language_name(span['language'])
        return expected
    commands = segments(record['command'])
    parts = record['parts']
    if commands is None or len(commands) != len(parts):
        raise ValueError(f'Parts do not align: {record["command"]!r}')
    joined = ''.join(parts)
    lead = len(joined) - len(joined.lstrip('\n')) if record['provider'] == 'claude' else 0
    offset = 0
    for command, part in zip(commands, parts):
        count = len(part.splitlines())
        language = pipeline_language(command)
        for index in range(max(0, offset - lead), min(len(rows), offset + count - lead)):
            expected[index] = language
        offset += count
    return expected


def score(records, observed, expanded=True, line_report=None):
    if len(records) != len(observed):
        raise ValueError('Rust report record count differs from input')
    totals = {provider: {'right': 0, 'wrong': 0, 'missed': 0} for provider in ('claude', 'codex')}
    wrong = []
    for record_index, (record, result) in enumerate(zip(records, observed), 1):
        if result['command'] != record['command'] or result['provider'] != record['provider']:
            raise ValueError('Rust report order differs from input')
        expected = [language_name(value, expanded) for value in truth(record)]
        actual = [None] * len(expected)
        for span in result['actual']:
            for index in range(max(0, span['start']), min(len(actual), span['end'])):
                actual[index] = language_name(span['language'], expanded)
        for index, (want, got) in enumerate(zip(expected, actual)):
            if want is None and got is None:
                continue
            verdict = 'right' if want == got else ('missed' if got is None else 'wrong')
            totals[record['provider']][verdict] += 1
            if line_report is not None:
                line_report.write(json.dumps({'record': record_index, 'provider': record['provider'],
                                             'line': index + 1, 'expected': want, 'actual': got,
                                             'verdict': verdict}, ensure_ascii=False) + '\n')
            if verdict == 'wrong':
                wrong.append({'provider': record['provider'], 'command': record['command'],
                              'line': index + 1, 'expected': want, 'actual': got,
                              'text': record['stdout'].splitlines()[index]})
    return {'providers': totals, 'wrong_lines': wrong}


def read_jsonl(path):
    with path.open() as source:
        return [json.loads(line) for line in source]


def highlight_counts(records, observed):
    if len(records) != len(observed):
        raise ValueError('Rust report record count differs from input')
    counts = []
    for record, result in zip(records, observed):
        if (result['command'], result['provider']) != (record['command'], record['provider']):
            raise ValueError('Rust report order differs from input')
        lines = set()
        total = len(record['stdout'].splitlines())
        for span in result['actual']:
            if not 0 <= span['start'] <= span['end'] <= total:
                raise ValueError('Rust report span is outside output')
            lines.update(range(span['start'], span['end']))
        counts.append(len(lines))
    return counts


def compare_highlights(records, counts, baseline):
    if len(counts) != len(baseline) or any(type(n) is not int or n < 0 for n in baseline):
        raise ValueError('Baseline must contain one nonnegative count per call')
    providers = {provider: {'before': 0, 'after': 0, 'delta': 0}
                 for provider in ('claude', 'codex')}
    regressions = []
    for index, (record, before, after) in enumerate(zip(records, baseline, counts), 1):
        totals = providers[record['provider']]
        totals['before'] += before
        totals['after'] += after
        totals['delta'] += after - before
        if after < before:
            regressions.append({'record': index, 'provider': record['provider'],
                                'command': record['command'], 'before': before,
                                'after': after, 'lost': before - after})
    return {'providers': providers, 'regressed_calls': regressions}


def check(args):
    for path in (args.calls, args.replay):
        if not path.is_file():
            raise ValueError(f'Input does not exist: {path}')
    args.report.parent.mkdir(parents=True, exist_ok=True)
    report = {}
    baseline = json.loads(args.baseline.read_text()) if args.baseline else None
    saved_baseline = {}
    failed = False
    started = time.monotonic()
    for name, path in (('calls', args.calls), ('replay', args.replay)):
        observed_path = args.report.with_name(args.report.stem + '-' + name + '.jsonl')
        log_path = args.report.with_name(args.report.stem + '-' + name + '.log')
        environment = dict(os.environ, SHELL_OUTPUT_INPUT=str(path.resolve()),
                           SHELL_OUTPUT_REPORT=str(observed_path.resolve()))
        command = ['cargo', 'test', '--release', 'shell_output_audit', '--', '--ignored', '--nocapture']
        with log_path.open('w') as log:
            process = subprocess.run(command, cwd=AGENTS, env=environment, stdout=log,
                                     stderr=subprocess.STDOUT, timeout=args.timeout, check=False)
        if process.returncode:
            raise RuntimeError(f'Rust audit failed ({process.returncode}); see {log_path}')
        records, observed = read_jsonl(path), read_jsonl(observed_path)
        if len(records) != len(observed):
            raise ValueError(f'Rust audit record count differs: {name}')
        timings = []
        for line in log_path.read_text().splitlines():
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(value, dict) and 'elapsed_seconds' in value:
                timings.append(value)
        if len(timings) != 1:
            raise ValueError(f'Rust audit must emit one timing record: {log_path}')
        timing = timings[0]
        if timing['calls'] != len(records):
            raise ValueError(f'Rust timing count differs: {name}')
        report[name] = {'count': len(records), 'elapsed_seconds': timing['elapsed_seconds']}
        counts = highlight_counts(records, observed)
        fingerprint = hashlib.sha256(path.read_bytes()).hexdigest()
        saved_baseline[name] = {'input_sha256': fingerprint, 'highlighted_lines': counts}
        report[name]['highlighted_lines'] = sum(counts)
        report[name]['providers'] = {
            provider: {'calls': sum(r['provider'] == provider for r in records),
                       'highlighted_lines': sum(n for r, n in zip(records, counts)
                                                if r['provider'] == provider)}
            for provider in ('claude', 'codex')}
        if baseline is not None:
            if baseline[name]['input_sha256'] != fingerprint:
                raise ValueError(f'Baseline input differs: {name}')
            comparison = compare_highlights(records, counts, baseline[name]['highlighted_lines'])
            report[name]['comparison'] = comparison
            failed |= bool(comparison['regressed_calls'])
        if name == 'replay':
            report[name]['reference'] = score(records, observed, expanded=False)
            line_path = args.report.with_name(args.report.stem + '-lines.jsonl')
            with line_path.open('w') as line_report:
                report[name]['scope'] = score(records, observed, line_report=line_report)
            report[name]['line_report'] = str(line_path)
            failed |= bool(report[name]['scope']['wrong_lines'])
        args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    report['elapsed_seconds'] = time.monotonic() - started
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    if args.write_baseline:
        args.write_baseline.parent.mkdir(parents=True, exist_ok=True)
        args.write_baseline.write_text(json.dumps(saved_baseline) + '\n')
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f'Audit including build: {report["elapsed_seconds"]:.3f}s', file=sys.stderr)
    return int(failed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    generator = sub.add_parser('generate', help='regenerate repository-only fixtures')
    generator.add_argument('--output', type=Path, default=DEFAULT_FIXTURE)
    generator.set_defaults(run=generate)
    audit = sub.add_parser('check', help='run and report the Rust corpus audit')
    audit.add_argument('--calls', type=Path, required=True)
    audit.add_argument('--replay', type=Path, required=True)
    audit.add_argument('--report', type=Path, required=True)
    audit.add_argument('--timeout', type=int, default=600)
    audit.add_argument('--baseline', type=Path,
                       help='fail if any call highlights fewer lines than this baseline')
    audit.add_argument('--write-baseline', type=Path,
                       help='save per-call highlighted line counts for these exact inputs')
    audit.set_defaults(run=check)
    args = parser.parse_args()
    return args.run(args)


if __name__ == '__main__':
    sys.exit(main())
