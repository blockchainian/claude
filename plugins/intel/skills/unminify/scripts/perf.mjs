#!/usr/bin/env node
import { readFileSync, realpathSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { intervalMs } from './timing.mjs';

const help = 'perf <dir> [--run-id <id>] [--window-seconds 10] [--slow-files 10]';

export function parseArgs(args) {
  const options = { windowSeconds: 10, slowFiles: 10 };
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (arg === '--help' || arg === '-h') return { help: true };
    if (arg.startsWith('-')) {
      const value = args[++index];
      if (!value || value.startsWith('--')) throw new Error(`Missing value for ${arg}`);
      if (arg === '--run-id') options.runId = value;
      else if (arg === '--window-seconds') {
        options.windowSeconds = Number(value);
        if (!Number.isFinite(options.windowSeconds) || options.windowSeconds < 0.001)
          throw new Error('--window-seconds must be at least 0.001');
      } else if (arg === '--slow-files') {
        options.slowFiles = Number(value);
        if (!Number.isSafeInteger(options.slowFiles) || options.slowFiles < 1)
          throw new Error('--slow-files must be a positive integer');
      } else throw new Error(`Unknown option: ${arg}`);
    } else if (options.dir) throw new Error(`Unexpected argument: ${arg}`);
    else options.dir = resolve(arg);
  }
  if (!options.dir) throw new Error(help);
  return options;
}

function readRecords(path, optional = false) {
  let contents;
  try { contents = readFileSync(path, 'utf8'); }
  catch (error) { if (optional && error.code === 'ENOENT') return []; throw error; }
  const lines = contents.split('\n');
  // Only newline-terminated appends are complete after an interrupted process.
  lines.pop();
  return lines.filter(line => line.trim()).map((line, index) => {
    try { return JSON.parse(line); }
    catch { throw new Error(`Invalid JSON in ${path}, record ${index + 1}`); }
  });
}

/** Summarize recorded attempts and lifecycle events; never connects to a model. */
export function report({ dir, runId, windowSeconds = 10, slowFiles = 10 }) {
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], { cwd: dir, encoding: 'utf8' }).trim();
  const performance = readRecords(join(root, '.unminify', 'performance.jsonl'));
  const start = runId === undefined
    ? performance.findLast(record => record.type === 'run_start')
    : performance.find(record => record.type === 'run_start' && record.run_id === runId);
  if (!start) throw new Error(runId === undefined ? 'No recorded run found.' : `Run not found: ${runId}`);
  const events = performance.filter(record => record.run_id === start.run_id);
  const requests = readRecords(join(root, '.unminify', 'requests.jsonl'), true)
    .filter(record => record.run_id === start.run_id);
  const summary = events.findLast(record => record.type === 'summary');
  const endedAt = Math.max(start.at, ...events.map(record => record.at), ...requests.map(record => record.ended_at));
  const width = windowSeconds * 1000;
  const windows = [];
  for (let at = start.at; at <= endedAt; at += width) {
    const end = at + width;
    const sent = requests.filter(request => request.sent_at >= at && request.sent_at < end);
    const completed = requests.filter(request => request.ended_at >= at && request.ended_at < end);
    const inFlight = requests.filter(request => request.sent_at < end && request.ended_at >= end).length;
    const activeAtStart = requests.filter(request => request.sent_at < at && request.ended_at > at).length;
    const peak = Math.max(activeAtStart, 0, ...sent.map(request => request.in_flight));
    windows.push({ started_at: at, ended_at: end, sent: sent.length, completed: completed.length,
      in_flight: inFlight, peak_in_flight: peak,
      average_duration_ms: completed.length ? completed.reduce((sum, request) => sum + request.duration_ms, 0) / completed.length : null,
      rate_limits: requests.filter(request => request.status === 429 && request.headers_at >= at && request.headers_at < end).length });
  }
  const phaseIntervals = {};
  const phaseStarts = new Map();
  for (const event of events) {
    const key = JSON.stringify([event.phase, event.file]);
    if (event.type === 'phase_start') {
      const starts = phaseStarts.get(key) ?? [];
      starts.push(event.at);
      phaseStarts.set(key, starts);
    } else if (event.type === 'phase_end') {
      const starts = phaseStarts.get(key);
      if (!starts?.length) throw new Error(`Phase ended without a start: ${event.phase}`);
      (phaseIntervals[event.phase] ??= []).push([starts.shift(), event.at]);
    }
  }
  const phaseMs = Object.fromEntries(Object.entries(phaseIntervals).map(([phase, intervals]) => [phase, intervalMs(intervals)]));
  const latestFiles = new Map();
  for (const event of events) if (event.type === 'file_end') latestFiles.set(event.file, event);
  const files = [...latestFiles.values()].map(event => ({ file: event.file,
    total_ms: event.at - event.started_at, last_batch_at: event.last_batch_at,
    written_at: event.written_at, cpu_ms: event.cpu_ms }));
  files.sort((a, b) => b.total_ms - a.total_ms || a.file.localeCompare(b.file));
  return { run_id: start.run_id, total_ms: summary ? summary.total_ms : endedAt - start.at,
    phase_ms: summary ? summary.phase_ms : phaseMs, windows, slow_files: files.slice(0, slowFiles) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  try {
    const options = parseArgs(process.argv.slice(2));
    console.log(options.help ? help : JSON.stringify(report(options), null, 2));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
