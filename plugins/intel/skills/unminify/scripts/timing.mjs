import { AsyncLocalStorage } from 'node:async_hooks';

const timings = new AsyncLocalStorage();
export const withTimings = (totals, task) => timings.run(totals, task);

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
  return { request_ms: { p50: percentile(0.5), p90: percentile(0.9), max: durations.at(-1) ?? 0 },
    average_in_flight: totalMs > 0 ? durations.reduce((sum, value) => sum + value, 0) / totalMs : 0,
    max_in_flight: peak, rate_limits: requests.filter(row => row.status === 429).length,
    pause_ms: intervalMs(pauses), requests_per_minute: totalMs > 0 ? requests.length * 60_000 / totalMs : 0 };
}
