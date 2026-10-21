#!/usr/bin/env python3
"""Run the requested gates with fixed deadlines and one completion signal."""
import json
import os
from pathlib import Path
import subprocess
import time

root = Path(__file__).resolve().parents[2]
out = Path(__file__).resolve().parent
env = dict(os.environ, CARGO_TARGET_DIR=str(root / 'target'))
results = []
for name, command in [
    ('nextest', ['cargo', 'nextest', 'run']),
    ('clippy', ['cargo', 'clippy', '--all-targets']),
    ('release', ['cargo', 'build', '--release']),
]:
    start = time.monotonic()
    with (out / f'{name}.log').open('w') as log:
        child = subprocess.Popen(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=600)
            state = 'completion' if code == 0 else 'failure'
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, 15)
            child.wait(timeout=10)
            code, state = 124, 'timeout'
    result = dict(check=name, state=state, exit_code=code, seconds=time.monotonic()-start)
    results.append(result)
    (out / 'checks.json').write_text(json.dumps(results, indent=2)+'\n')
    if code:
        print(json.dumps(result), flush=True)
        raise SystemExit(code)
print(json.dumps(dict(state='completion', checks=results)), flush=True)
