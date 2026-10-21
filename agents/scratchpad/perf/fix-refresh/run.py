#!/usr/bin/env python3
"""Fixed-deadline monitor: one final completion/failure/timeout signal."""
import argparse
import json
import os
import pathlib
import signal
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('label')
parser.add_argument('--timeout', type=float, default=360)
parser.add_argument('command', nargs=argparse.REMAINDER)
args = parser.parse_args()
out = pathlib.Path(__file__).resolve().parent
start = time.monotonic()
with (out / (args.label + '.log')).open('w') as log:
    process = subprocess.Popen(args.command, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        returncode = process.wait(timeout=args.timeout)
        result = {'status': 'completion' if returncode == 0 else 'failure', 'returncode': returncode, 'elapsed_s': time.monotonic() - start}
    except subprocess.TimeoutExpired:
        # The Python probe handles SIGTERM in finally and reaps its own PTY child.
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        result = {'status': 'timeout', 'returncode': process.returncode, 'elapsed_s': time.monotonic() - start}
(out / (args.label + '.status.json')).write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result))
raise SystemExit(0 if result['status'] == 'completion' else 1)
