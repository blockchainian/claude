import { AsyncLocalStorage } from 'node:async_hooks';

const timings = new AsyncLocalStorage();
export const withTimings = (totals, task) => timings.run(totals, task);

/** Record worker measurements in the file context that submitted its task. */
export function captureTimings() {
  const totals = timings.getStore();
  return values => {
    if (totals) for (const [step, elapsed] of Object.entries(values)) totals[step] = (totals[step] ?? 0) + elapsed;
  };
}

// Local elapsed time, including child processes; nested steps are inclusive.
export function timed(step, task) {
  const started = performance.now();
  try { return task(); }
  finally {
    const totals = timings.getStore();
    if (totals) totals[step] = (totals[step] ?? 0) + performance.now() - started;
  }
}

export function intervalMs(intervals) {
  let total = 0, end = -Infinity;
  for (const [start, finish] of [...intervals].sort((a, b) => a[0] - b[0])) {
    total += Math.max(0, finish - Math.max(start, end));
    end = Math.max(end, finish);
  }
  return total;
}

export function requestStats(requests, pauses, totalMs) {
  const durations = requests.map(row => row.duration_ms).sort((a, b) => a - b);
  const percentile = fraction => durations.length ? durations[Math.ceil(durations.length * fraction) - 1] : 0;
  const peak = Math.max(0, ...requests.map(row => row.in_flight ?? 0));
  return { ...namingStats(requests), request_ms: { p50: percentile(0.5), p90: percentile(0.9), max: durations.at(-1) ?? 0 },
    average_in_flight: totalMs > 0 ? durations.reduce((sum, value) => sum + value, 0) / totalMs : 0,
    max_in_flight: peak, rate_limits: requests.filter(row => row.status === 429).length,
    pause_ms: intervalMs(pauses), requests_per_minute: totalMs > 0 ? requests.length * 60_000 / totalMs : 0 };
}

export function namingStats(requests) {
  const model = requests.filter(row => row.kind === 'model');
  const counts = model.map(row => row.name_count).filter(Number.isFinite).sort((a, b) => a - b);
  const middle = Math.floor(counts.length / 2);
  return { requests: model.length,
    estimated_input_tokens: model.reduce((sum, row) => sum + (row.estimated_input_tokens ?? 0), 0),
    input_tokens: model.reduce((sum, row) => sum + (row.usage?.input_tokens ?? 0), 0),
    names_per_request: {
    median: counts.length ? (counts[Math.floor((counts.length - 1) / 2)] + counts[middle]) / 2 : 0,
    mean: counts.length ? counts.reduce((sum, count) => sum + count, 0) / counts.length : 0,
  } };
}
