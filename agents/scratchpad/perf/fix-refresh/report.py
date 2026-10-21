#!/usr/bin/env python3
"""Render complete independent probe summaries; refuse mixed or missing sources."""
import argparse
import json
import math
import pathlib
import re

parser = argparse.ArgumentParser()
parser.add_argument('labels', nargs='+')
args = parser.parse_args()
out = pathlib.Path(__file__).resolve().parent
rows = []
for label in args.labels:
    modes = {}
    for mode in ('startup', 'interaction', 'exit'):
        path = out / (label + '-' + mode + '-summary.json')
        if not path.exists():
            raise SystemExit(f'Missing probe mode: {path}')
        data = json.loads(path.read_text())
        if data['status'] != 'completion' or not data['runs']:
            raise SystemExit(f'Incomplete mode: {path}')
        modes[mode] = data
    metadata = modes['startup']['metadata']
    if not re.fullmatch('[0-9a-f]{40}', metadata['source_head'] or ''):
        raise SystemExit(f'{label}: explicit 40-character --source-head required for a performance report')
    if any(data['metadata'] != metadata for data in modes.values()):
        raise SystemExit(f'{label}: binary or metadata differ between modes')
    provenance_path = out / f'{label}-source.json'
    if not provenance_path.exists():
        raise SystemExit(f'Missing source provenance: {provenance_path}')
    provenance = json.loads(provenance_path.read_text())
    if provenance['binary_sha256'] != metadata['sha256']:
        raise SystemExit(f'{label}: binary checksum differs from source provenance')
    source_head = provenance['base'] if 'patch_sha256' in provenance else provenance['source_head']
    if source_head != metadata['source_head']:
        raise SystemExit(f'{label}: source HEAD differs from source provenance')
    exits = modes['startup']['runs'] + modes['exit']['runs'] + [modes['interaction']['idle_exit']]
    if any(not row['exited'] or row['exit_code'] != 0 for row in exits):
        raise SystemExit(f'{label}: at least one Ctrl+C did not exit successfully')
    if modes['interaction']['all_actions']['settle_ms']['n'] == 0:
        raise SystemExit(f'{label}: no substantive navigation responses')
    rows.append((label, modes))
if len({modes['startup']['metadata']['source_head'] for _, modes in rows}) != 1:
    raise SystemExit('Before/after source HEAD differs; build both from the same git archive HEAD')
lines = ['# Refresh input latency measurements', '', 'Real local HOME, 200 × 50 PTY. Navigation only; no prompt submissions. No tmux server was created.', '',
'Each action follows a 300 ms PTY output drain. First-byte timing is write → the first PTY read that brings cumulative response above six bytes, ignoring a possible lone hide-cursor sequence; timestamps include terminal decoding. It does not identify the input-acknowledging frame. Burst-settle timing is write → the last observed PTY output before 60 ms without output, with a 2.5 s observation limit. These are different endpoints.', '',
'Percentiles use nearest rank. All 150 actions remain included, including any observation-limit tail. Keyboard, click and wheel groups use their full respective 60 / 30 / 60 actions.', '', '| Metric | Historical report | ' + ' | '.join(label for label, _ in rows) + ' |', '|---|---|' + '---|' * len(rows)]
def metric(name, historical, function):
    lines.append('| ' + name + ' | ' + historical + ' | ' + ' | '.join(str(function(modes)) for _, modes in rows) + ' |')
def nums(values):
    return ', '.join(f'{value:.2f}' for value in values)
metric('First footer frame, ms', 'first content: 1851–1874', lambda modes: nums([r['frame_ms'] for r in modes['startup']['runs']]))
metric('First visible session row, ms', 'not separately measured', lambda modes: nums([r['loaded_ms'] for r in modes['startup']['runs']]))
metric('First Active/Inactive header, ms', '1851–1874', lambda modes: nums([r['content_ms'] for r in modes['startup']['runs'] if r['content_ms'] is not None]))
for q in (50, 80, 99):
    metric(f'Key/mouse burst settle p{q}, ms', {50: 'under 10', 80: '662', 99: '951'}[q], lambda modes, q=q: modes['interaction']['all_actions']['settle_ms'][f'p{q}_ms'])
def grouped(modes, prefix):
    values = sorted(row['settle_ms'] for row in modes['interaction']['runs'] if row['action'].startswith(prefix) and row['first_ms'] is not None)
    return ' / '.join(f'{values[math.ceil(len(values) * q / 100) - 1]:.2f}' for q in (50, 80, 99)) if values else 'no responses'
for title, prefix in [('Keyboard', 'key-'), ('Click', 'click-'), ('Wheel', 'wheel-')]:
    metric(title + ' p50 / p80 / p99, ms', 'not separately reported', lambda modes, prefix=prefix: grouped(modes, prefix))
metric('First byte p50 / p80 / p99, ms', 'not reported', lambda modes: ' / '.join(f"{modes['interaction']['all_actions']['first_ms'][f'p{q}_ms']:.2f}" for q in (50, 80, 99)))
metric('Navigation actions with response', 'not given', lambda modes: modes['interaction']['all_actions']['settle_ms']['n'])
metric('Navigation actions without response', 'not given', lambda modes: modes['interaction']['actions_with_no_substantive_output'])
metric('Ctrl+C during startup window, ms', 'not measured', lambda modes: nums([row['exit_ms'] for row in modes['exit']['runs']]))
metric('Ctrl+C after interaction, ms', 'not measured', lambda modes: modes['interaction']['idle_exit']['exit_ms'])
for label, modes in rows:
    metadata = modes['startup']['metadata']
    lines += ['', f'## {label}', '', f'- Binary: `{metadata["binary"]}`.', f'- SHA256: `{metadata["sha256"]}`.', f'- Explicit archive source HEAD: `{metadata["source_head"]}`.', f'- Provider/home overrides: `{json.dumps(metadata["overrides"])}`.', '', '| Action | n | p50 ms | p80 ms | p99 ms |', '|---|---:|---:|---:|---:|']
    for name, entry in modes['interaction']['by_action'].items():
        values = entry['settle_ms']
        lines.append('| ' + name + ' | ' + ' | '.join(str(values.get(key, '—')) for key in ('n', 'p50_ms', 'p80_ms', 'p99_ms')) + ' |')
    click_rows = [row for row in modes['interaction']['runs'] if row['action'] == 'click-task']
    if not all(row['click_y'] != row['selected_y_before'] for row in click_rows):
        raise SystemExit(f'{label}: a click targeted the selected task row')
    limit_hits = [(index + 1, row) for index, row in enumerate(modes['interaction']['runs']) if row['settle_ms'] >= 2500]
    for index, row in limit_hits:
        lines += ['', f'- Observation-limit tail retained: action {index} ({row["action"]}) first response {row["first_ms"]} ms, last observed output {row["settle_ms"]} ms, {row["bytes"]} bytes. The probe reached its 2.5 s limit before observing 60 ms quiet. This is a bounded observation, not a measured completed settle or proof of blocked input. No per-action stack/frame evidence identifies the output source.']
    lines += ['', '- Click strategy: different visible leaf task; every target differs from the selected cursor row. Click coordinates and target text are saved in the interaction summary.', '- Startup interrupt sample evidence:']
    for index, row in enumerate(modes['exit']['runs']):
        path = out / f'{label}-exit-{index}.sample.txt'
        current = None
        threads = set()
        if path.exists():
            for raw_line in path.read_text().splitlines():
                if re.match(r'^\s*\d+ Thread_', raw_line):
                    current = raw_line.strip()
                if any(marker in raw_line for marker in ('Hub7refresh', 'Hub16refresh_selected', 'Hub::refresh')) and current:
                    threads.add(current)
        if threads:
            main = any('main-thread' in thread for thread in threads)
            worker = any('agents-refresh' in thread for thread in threads)
            lines.append(f'  - Run {index + 1}: refresh observed; main thread={main}, agents-refresh worker={worker}; sample return code={row.get("sample_returncode")}.')
        else:
            lines.append(f'  - Run {index + 1}: refresh state inconclusive; sample file exists={path.exists()}, return code={row.get("sample_returncode")}.')
    provenance = out / f'{label}-source.json'
    if provenance.exists():
        source = json.loads(provenance.read_text())
        if 'patch_sha256' in source:
            lines.append(f'- Source: the same frozen archive plus own patch SHA256 `{source["patch_sha256"]}`; provenance `{provenance.name}`.')
        elif 'archive_sha256' in source:
            lines.append(f'- Source: unmodified archive SHA256 `{source["archive_sha256"]}`; provenance `{provenance.name}`.')

lines += ['', '## Limits', '', '- Historical numbers came from another source/data state and a broader interaction workload. The current before/after workload uses the same navigation actions.', '- PTY byte timing can include unrelated refresh output; it does not identify the precise frame acknowledging a key. Actions without substantive output are counted separately and excluded from percentiles.', '- Active/Inactive header presence measures a frame, not loaded session rows. The interaction mode waits after the footer and rejects a screen with no selected task.', '- Startup interrupt timing alone does not prove the refresh was running at the exact key timestamp. Optional macOS sample records refresh in an overlapping window; missing/failed samples must be reported as inconclusive.', '- Live writers and concurrent system load affect measurements. The explicit source SHA and binary checksum are recorded; source patch and archive construction evidence must accompany this report.', '']
lines += ['', '## Refresh blocked by real SQLite lock', '', 'The regression `refresh_allows_first_frame_input_and_exit_while_sqlite_is_locked` uses a real SQLite connection and `BEGIN EXCLUSIVE` with an 800 ms release timeout. It starts refresh, draws, handles Ctrl+C and drops App; the measured path must finish within 300 ms while the database remains exclusively locked and the refresh result has not been awaited. The test failed before with `895.717417ms` (`red.log`) and passes in the captured 24-test focused run (`candidate-final-host.log`, whole test duration 0.051 s). Whole-test duration is not a separate input latency measurement.', '', 'This deterministic lock regression complements the host PTY exit samples. If a fast exit prevents macOS sample from attaching or recording refresh, the corresponding host run does not prove refresh was in flight at the precise Ctrl+C timestamp.', '', '## Source and exclusions', '', 'The comparison binaries both start from frozen source 9af4e1ee35473c94e6441de5e792f32ddab09c74. This performance comparison covers that archive plus the recorded own patch, separately from any later integration gates on a newer HEAD.', '', 'Excluded data: sandbox-failed/, unverified before-agents runs, script-smoke*/, candidate-* and baseline-interaction-fixed-coordinate-summary.json. They are retained only as probe diagnostics. No tmux server was created.', '']
for label, modes in rows:
    if label != 'after':
        continue
    startup = modes['startup']
    interaction = modes['interaction']
    exits = modes['exit']
    metadata = startup['metadata']
    source = json.loads((out / 'after-source.json').read_text())
    def stats(values):
        values = sorted(values)
        if not values:
            return {'n': 0}
        return {'n': len(values), **{f'p{q}_ms': values[math.ceil(len(values) * q / 100) - 1] for q in (50, 80, 99)}, 'max_ms': max(values)}
    grouped_actions = {name: {metric: stats([row[metric] for row in interaction['runs'] if row['action'].startswith(prefix) and row['first_ms'] is not None]) for metric in ('first_ms', 'settle_ms')} for name, prefix in [('keys', 'key-'), ('clicks', 'click-'), ('wheels', 'wheel-')]}
    clicks = [row for row in interaction['runs'] if row['action'] == 'click-task']
    summary = {
        'status': 'final', 'source_head': metadata['source_head'], 'own_patch_sha256': source['patch_sha256'],
        'binary': metadata['binary'], 'binary_sha256': metadata['sha256'], 'measurement_cwd': metadata['measurement_cwd'],
        'first_footer_frame_ms': [row['frame_ms'] for row in startup['runs']],
        'first_session_row_ms': [row['loaded_ms'] for row in startup['runs']],
        'first_footer_frame': stats([row['frame_ms'] for row in startup['runs']]),
        'first_session_row': stats([row['loaded_ms'] for row in startup['runs']]),
        'all_actions': interaction['all_actions'], 'grouped_actions': grouped_actions, 'by_action': interaction['by_action'],
        'actions_without_output': interaction['actions_with_no_substantive_output'],
        'clicks_target_different_row': all(row['click_y'] != row['selected_y_before'] for row in clicks),
        'click_response_n': interaction['by_action']['click-task']['settle_ms']['n'],
        'click_response_bytes_min': min(row['bytes'] for row in clicks),
        'startup_ctrl_c_ms': [row['exit_ms'] for row in exits['runs']], 'idle_ctrl_c_ms': interaction['idle_exit']['exit_ms'],
        'exit_samples': exits['runs'], 'tmux_created': False,
        'settle_limit_hit_rows': [dict(action_number=index + 1, **row) for index, row in enumerate(interaction['runs']) if row['settle_ms'] >= 2500],
        'percentile_policy': 'nearest rank; all 150 actions included, including observation-limit tails',
        'timing_notes': {'first_ms': 'first PTY read that brings cumulative response above six bytes, after terminal decoding; not frame acknowledgement',
                         'settle_ms': 'last output before 60 ms quiet, observed for at most 2.5 s; an observation-limit tail is not a completed settle'},
        'verification': {'all_modes_complete': all(data['status'] == 'completion' for data in modes.values()),
                         'all_150_actions_responded': len(interaction['runs']) == 150 and interaction['all_actions']['settle_ms']['n'] == 150 and interaction['actions_with_no_substantive_output'] == 0,
                         'all_three_samples_show_refresh_only_on_worker': len(exits['runs']) == 3 and all(row.get('refresh_on_worker_thread') is True and row.get('refresh_on_main_thread') is False for row in exits['runs'])}}
    (out / 'after-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
(out / 'report.md').write_text('\n'.join(lines))
print(out / 'report.md')
