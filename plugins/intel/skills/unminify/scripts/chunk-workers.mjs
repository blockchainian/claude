import { Worker } from 'node:worker_threads';
import { availableParallelism, totalmem } from 'node:os';
import { captureTimings } from './timing.mjs';

/** Each isolate owns one chunk and exits before its scheduler slot is reused. */
export async function runChunkWorkers(jobs, { root, settings, selected, fragmentContext,
  namer, withSlot, emit, phase, summary, sources, size = Math.max(1,
    Math.min(availableParallelism(), Math.floor(totalmem() / (8 * 1024 ** 3)), 8)) }) {
  if (!Number.isSafeInteger(size) || size < 1) throw new RangeError('chunk worker size must be a positive integer');
  const results = [], pending = jobs.values();
  let failure;
  function execute(job) {
    const recordTimings = captureTimings();
    return new Promise((resolve, reject) => {
      const worker = new Worker(new URL('./chunk-worker.mjs', import.meta.url), {
        execArgv: [],
        workerData: { root, settings, selected, fragmentContext, job },
        resourceLimits: { maxOldGenerationSizeMb: 3072 },
      });
      const phases = new Map();
      let finished = false;
      async function finish(error, result) {
        if (finished) return;
        finished = true;
        if (error) failure ??= error;
        try {
          for (const end of phases.values()) end();
          phases.clear();
          await worker.terminate();
          if (error) reject(error); else resolve(result);
        } catch (cleanupError) { reject(error ?? cleanupError); }
      }
      worker.on('message', message => {
        if (finished) return;
        try {
          if (message.type === 'name') {
            Promise.resolve().then(() => withSlot(() => namer.name(message.payload, message.metadata)))
              .then(result => { if (!finished) worker.postMessage({ id: message.id, result }); },
                error => { if (!finished) worker.postMessage({ id: message.id,
                  error: { message: error.message, stack: error.stack, code: error.code } }); })
              .catch(error => finish(error));
          } else if (message.type === 'event') emit(message.event);
          else if (message.type === 'phase-start') phases.set(message.id, phase(message.name, message.file));
          else if (message.type === 'phase-end') {
            const end = phases.get(message.id);
            if (!end) throw new Error(`unknown chunk worker phase: ${message.id}`);
            phases.delete(message.id);
            end();
          } else if (message.type === 'done' || message.type === 'failed') {
            for (const [key, value] of Object.entries(message.summary)) summary[key] = (summary[key] ?? 0) + value;
            for (const [file, source] of message.sources) sources.set(file, source);
            recordTimings(message.cpu_ms);
            emit({ chunk: job.id, status: 'cpu-timing', cpu_ms: message.cpu_ms, threadId: message.threadId });
            finish(message.error ? Object.assign(new Error(message.error.message), message.error) : undefined, message);
          } else throw new Error(`unknown chunk worker message: ${message.type}`);
        } catch (error) { finish(error); }
      });
      worker.once('error', error => finish(error));
      worker.once('exit', code => {
        if (!finished) finish(new Error(`chunk worker exited before completion (${code}): ${job.id}`));
      });
    });
  }
  async function consume() {
    while (!failure) {
      const next = pending.next();
      if (next.done) return;
      try { results.push(await execute(next.value)); }
      catch (error) { failure ??= error; }
    }
  }
  await Promise.all(Array.from({ length: Math.min(size, jobs.length) }, consume));
  if (failure) throw failure;
  return results;
}
