// ABOUTME: Runs an async worker over a list with a cap on how many run at once.
// ABOUTME: Drives multiple login flows in parallel up to --concurrency; results keep input order.

// Runs `worker(item, index)` over every item, keeping at most `limit` in flight at a time, and
// returns the results in input order. A bad or missing limit falls back to 1 (sequential). The
// worker's own errors propagate; the login loop's worker catches per account so one failure can't
// stop the rest.
export async function runWithConcurrency(items, limit, worker) {
  const cap = Number.isFinite(limit) && limit >= 1 ? Math.floor(limit) : 1;
  const max = Math.min(cap, items.length);
  const results = new Array(items.length);
  let next = 0;
  async function runner() {
    while (true) {
      const i = next++;
      if (i >= items.length) return;
      results[i] = await worker(items[i], i);
    }
  }
  await Promise.all(Array.from({ length: max }, runner));
  return results;
}
