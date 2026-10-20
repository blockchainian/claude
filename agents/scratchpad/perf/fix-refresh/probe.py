#!/usr/bin/env python3
"""Real HOME PTY latency probe. Adapted from interaction and startup-slow probes.

Only local task navigation; no prompt text, submit, or provider actions.
Each mode writes an independent summary. EOF is normal during Ctrl+C drain/reap.
"""
import argparse
import codecs
import errno
import fcntl
import hashlib
import json
import math
import os
import pathlib
import pty
import random
import re
import select
import signal
import struct
import subprocess
import termios
import time

import pyte


def quantiles(values):
    values = sorted(values)
    if not values:
        return {'n': 0}
    return {'n': len(values), **{f'p{q}_ms': values[max(0, math.ceil(len(values) * q / 100) - 1)] for q in (50, 80, 99)}, 'max_ms': max(values)}


class App:
    def __init__(self, binary, out, name, timeout):
        self.out, self.name = out, name
        self.start = time.monotonic()
        self.deadline = self.start + timeout
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ['TERM'] = 'xterm-256color'
            os.execv(str(binary), [str(binary)])
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 50, 200, 0, 0))
        self.screen = pyte.Screen(200, 50)
        self.stream = pyte.Stream(self.screen)
        self.decoder = codecs.getincrementaldecoder('utf-8')('replace')
        self.raw = bytearray()
        self.replied = False
        self.frame = self.content = self.loaded = None
        self.first_session_row = None
        self.status = None
        self.eof = False
        self.read_errors = []

    def exited(self):
        if self.status is not None:
            return True
        pid, status = os.waitpid(self.pid, os.WNOHANG)
        if pid:
            self.status = status
        return bool(pid)

    def text(self):
        return '\n'.join(self.row_text(row) for row in range(self.screen.lines))

    def row_text(self, row):
        # pyte.display indexes empty wide-character continuation cells; read cells
        # directly, as the original interaction probe does.
        return ''.join(self.screen.buffer[row][column].data or ' ' for column in range(self.screen.columns))

    def pump(self, wait):
        if self.eof:
            # PTY EOF can precede the child's wait status by a few milliseconds.
            time.sleep(min(wait, .002))
            return 0
        if not select.select([self.fd], [], [], max(0, wait))[0]:
            return 0
        try:
            chunk = os.read(self.fd, 1 << 20)
        except OSError as error:
            if error.errno == errno.EIO:
                self.eof = True
                return 0
            self.read_errors.append(repr(error))
            raise
        if not chunk:
            self.eof = True
            return 0
        self.raw.extend(chunk)
        self.stream.feed(self.decoder.decode(chunk))
        if not self.replied and b'\x1b[c' in self.raw:
            os.write(self.fd, b'\x1b]11;rgb:1111/1111/1111\x1b\\\x1b[?62;22;52c')
            self.replied = True
        if self.frame is None and 'help' in self.row_text(self.screen.lines - 1):
            self.frame = time.monotonic() - self.start
        if self.content is None and b'Active' in self.raw and b'Inactive' in self.raw:
            self.content = time.monotonic() - self.start
        if self.loaded is None:
            rows = [self.row_text(row)[:50] for row in range(2, 29)]
            session_row = next((row for row in rows if '●' in row or '○' in row), None)
            if session_row is not None:
                self.loaded = time.monotonic() - self.start
                self.first_session_row = session_row.strip()
        return len(chunk)

    def check(self):
        if self.exited():
            raise RuntimeError(f'{self.name}: process exited unexpectedly, wait status={self.status}')
        if time.monotonic() > self.deadline:
            raise TimeoutError(f'{self.name}: fixed process deadline reached')
        if b'agents: Codex: Operation not permitted' in self.raw:
            raise RuntimeError(f'{self.name}: real session discovery blocked; measurement invalid')

    def drain(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.check()
            self.pump(min(.01, max(0, end - time.monotonic())))

    def frame_ready(self):
        while self.frame is None:
            self.check()
            self.pump(.01)
        self.check()

    def loaded_ready(self):
        self.frame_ready()
        while self.loaded is None:
            self.check()
            self.pump(.01)

    def interaction_ready(self, seconds):
        self.loaded_ready()
        self.drain(seconds)
        if 'No task' in self.text():
            raise RuntimeError(f'{self.name}: no selected real task; navigation measurement invalid')

    def interrupt(self, timeout=10):
        if self.exited():
            raise RuntimeError(f'{self.name}: exited before Ctrl+C')
        start = time.monotonic()
        os.write(self.fd, b'\x03')
        deadline = start + timeout
        while time.monotonic() < deadline:
            # Reap and drain independently: EOF is expected here, never an exception.
            if self.exited():
                return {'exit_ms': round((time.monotonic() - start) * 1000, 2), 'exited': True, 'wait_status': self.status, 'exit_code': os.waitstatus_to_exitcode(self.status), 'pty_eof': self.eof}
            self.pump(.002)
        return {'exit_ms': round((time.monotonic() - start) * 1000, 2), 'exited': False, 'wait_status': None, 'exit_code': None, 'pty_eof': self.eof}

    def close(self):
        try:
            (self.out / (self.name + '.ansi')).write_bytes(self.raw)
            (self.out / (self.name + '.screen.txt')).write_text(self.text() + '\n')
        finally:
            # A diagnostic/rendering failure must not strand the owned PTY child.
            if not self.exited():
                os.kill(self.pid, signal.SIGTERM)
                deadline = time.monotonic() + 2
                while not self.exited() and time.monotonic() < deadline:
                    self.pump(.002)
                if self.status is None:
                    os.kill(self.pid, signal.SIGKILL)
                    _, self.status = os.waitpid(self.pid, 0)
            os.close(self.fd)
            (self.out / (self.name + '.diagnostics.json')).write_text(json.dumps({'wait_status': self.status, 'exit_code': os.waitstatus_to_exitcode(self.status), 'pty_eof': self.eof, 'read_errors': self.read_errors, 'bytes': len(self.raw)}, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('binary', type=pathlib.Path)
    parser.add_argument('output', type=pathlib.Path)
    parser.add_argument('label')
    parser.add_argument('--mode', choices=['all', 'startup', 'interaction', 'exit'], default='all')
    parser.add_argument('--source-head', help='Explicit source SHA of the archive/copy used to build this binary')
    parser.add_argument('--actions', type=int, default=100)
    parser.add_argument('--pause-min', type=float, default=.3)
    parser.add_argument('--pause-max', type=float, default=.3)
    parser.add_argument('--startup-runs', type=int, default=3)
    parser.add_argument('--exit-runs', type=int, default=3)
    parser.add_argument('--ready-wait', type=float, default=3)
    parser.add_argument('--exit-delay', type=float, default=.4)
    parser.add_argument('--process-timeout', type=float, default=120)
    parser.add_argument('--sample', action='store_true', help='Capture an overlapping macOS sample for startup exit runs')
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    args.output.mkdir(parents=True, exist_ok=True)
    args.binary = args.binary.resolve()
    metadata = {'binary': str(args.binary), 'sha256': hashlib.sha256(args.binary.read_bytes()).hexdigest(), 'source_head': args.source_head, 'source_head_provided': args.source_head is not None, 'real_home': str(pathlib.Path.home()), 'measurement_cwd': str(pathlib.Path.cwd()), 'overrides': {key: os.environ.get(key) for key in ['CLAUDE_CONFIG_DIR', 'CODEX_HOME', 'CODEX_MANAGER_STATE_DIR']}, 'method': f'200x50 PTY; OSC11/DA1 replies; write to first substantive bytes and last burst byte; pre-action drain {args.pause_min}-{args.pause_max} s; 60 ms quiet; nearest-rank percentiles'}
    def save(mode, data):
        (args.output / (args.label + '-' + mode + '-summary.json')).write_text(json.dumps(data, indent=2) + '\n')
    def app(mode, index):
        return App(args.binary, args.output, args.label + '-' + mode + '-' + str(index), args.process_timeout)
    for mode in ['startup', 'interaction', 'exit'] if args.mode == 'all' else [args.mode]:
        result = {'metadata': metadata, 'mode': mode, 'status': 'running', 'runs': []}
        save(mode, result)
        try:
            if mode == 'startup':
                for index in range(args.startup_runs):
                    child = app(mode, index)
                    try:
                        child.loaded_ready()
                        child.drain(.1)
                        row = {'frame_ms': round(child.frame * 1000, 2), 'content_ms': None if child.content is None else round(child.content * 1000, 2), 'loaded_ms': round(child.loaded * 1000, 2), 'first_session_row': child.first_session_row, **child.interrupt()}
                        result['runs'].append(row)
                    finally:
                        child.close()
                    save(mode, result)
                result['frame'] = quantiles([row['frame_ms'] for row in result['runs']])
            elif mode == 'interaction':
                child = app(mode, 0)
                def mouse(button, x, y, up=False):
                    return f'\x1b[<{button};{x};{y}{"m" if up else "M"}'.encode()
                actions = [('key-task-down', b'\x1b[B'), ('key-task-up', b'\x1b[A'), ('click-task', mouse(0, 15, 8) + mouse(0, 15, 8, True)), ('wheel-task-down', mouse(65, 15, 10)), ('wheel-task-up', mouse(64, 15, 10))]
                rng = random.Random(48291)
                result['click_strategy'] = 'different-visible-leaf-task'
                try:
                    child.interaction_ready(args.ready_wait)
                    for index in range(args.actions):
                        child.drain(rng.uniform(args.pause_min, args.pause_max))
                        name, data = actions[index % len(actions)]
                        click = {}
                        if name == 'click-task':
                            visible = []
                            for row in range(2, 29):
                                text = child.row_text(row)[:50]
                                icons = [text.find(icon) for icon in ('●', '○') if icon in text]
                                if icons:
                                    visible.append((row, text, min(icons)))
                            selected = child.screen.cursor.y
                            if selected not in [row for row, _, _ in visible]:
                                raise RuntimeError('Cannot identify selected task cursor before click')
                            # A visible parent header is not selectable. Choose a leaf whose
                            # following task is at the same or shallower tree depth.
                            candidates = [(row, text) for position, (row, text, depth) in enumerate(visible[:-1])
                                          if visible[position + 1][2] <= depth and row != selected]
                            if not candidates:
                                raise RuntimeError('No distinct selectable visible task for click')
                            target, target_text = candidates[(index // len(actions)) % len(candidates)]
                            data = mouse(0, 15, target + 1) + mouse(0, 15, target + 1, True)
                            click = {'click_x': 15, 'click_y': target + 1,
                                     'selected_y_before': selected + 1, 'target_row': target_text.strip()}
                        start = time.monotonic()
                        os.write(child.fd, data)
                        first, last, size = None, start, 0
                        while time.monotonic() - start < 2.5:
                            child.check()
                            count = child.pump(.002)
                            if count:
                                size += count
                                last = time.monotonic()
                                if first is None and size > 6:
                                    first = last - start
                            elif first is not None and time.monotonic() - last > .06:
                                break
                        result['runs'].append({'action': name, 'sent_at_s': round(start - child.start, 4), 'first_ms': None if first is None else round(first * 1000, 2), 'settle_ms': round((last - start) * 1000, 2), 'bytes': size, **click, **({'selected_y_after': child.screen.cursor.y + 1} if click else {})})
                        save(mode, result)
                    result['idle_exit'] = child.interrupt()
                finally:
                    child.close()
                result['all_actions'] = {metric: quantiles([row[metric] for row in result['runs'] if row['first_ms'] is not None]) for metric in ['first_ms', 'settle_ms']}
                result['by_action'] = {name: {metric: quantiles([row[metric] for row in result['runs'] if row['action'] == name and row['first_ms'] is not None]) for metric in ['first_ms', 'settle_ms']} for name, _ in actions}
                result['actions_with_no_substantive_output'] = sum(row['first_ms'] is None for row in result['runs'])
            else:
                for index in range(args.exit_runs):
                    child = app(mode, index)
                    sampler = log = None
                    try:
                        path = args.output / (child.name + '.sample.txt')
                        if args.sample:
                            log = (args.output / (child.name + '.sample.log')).open('w')
                            sampler = subprocess.Popen(['/usr/bin/sample', str(child.pid), '1', '1', '-file', str(path)], stdout=log, stderr=subprocess.STDOUT)
                        child.drain(args.exit_delay)
                        sent = time.monotonic() - child.start
                        row = {'ctrl_c_at_s': round(sent, 4), **child.interrupt()}
                        if sampler is not None:
                            try:
                                sampler.wait(timeout=10)
                            except subprocess.TimeoutExpired:
                                sampler.terminate()
                                sampler.wait(timeout=2)
                            sample = path.read_text() if path.exists() else ''
                            current_thread = None
                            refresh_threads = set()
                            for sample_line in sample.splitlines():
                                if re.match(r'^\s*\d+ Thread_', sample_line):
                                    current_thread = sample_line.strip()
                                if any(marker in sample_line for marker in ('Hub7refresh', 'Hub16refresh_selected', 'Hub::refresh')) and current_thread:
                                    refresh_threads.add(current_thread)
                            row.update({'sample_returncode': sampler.returncode, 'refresh_in_overlapping_sample': bool(refresh_threads) if path.exists() else None,
                                        'refresh_on_main_thread': any('main-thread' in thread for thread in refresh_threads) if path.exists() else None,
                                        'refresh_on_worker_thread': any('agents-refresh' in thread for thread in refresh_threads) if path.exists() else None,
                                        'refresh_threads': sorted(refresh_threads), 'sample_exists': path.exists()})
                        else:
                            row['refresh_in_overlapping_sample'] = None
                        result['runs'].append(row)
                    finally:
                        if sampler is not None and sampler.poll() is None:
                            sampler.kill()
                            sampler.wait()
                        if log is not None:
                            log.close()
                        child.close()
                    save(mode, result)
                result['exit'] = quantiles([row['exit_ms'] for row in result['runs']])
            result['status'] = 'completion'
        except BaseException as error:
            result['status'] = 'failure'
            result['error'] = repr(error)
            raise
        finally:
            save(mode, result)
        print(json.dumps({'mode': mode, 'status': result['status'], 'runs': len(result['runs'])}), flush=True)


if __name__ == '__main__':
    main()
