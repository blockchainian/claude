import { availableParallelism } from 'node:os';
import { Worker } from 'node:worker_threads';

/** Lazily allocate workers; each owns at most one syntax tree at a time. */
export function createSyntaxPool({ size = availableParallelism(), onWorker } = {}) {
  if (!Number.isSafeInteger(size) || size < 1) throw new Error('syntax pool size must be a positive integer');
  const workers = [], queue = [];
  let closed = false, failure;
  function fail(error) {
    failure ??= error;
    for (const job of queue.splice(0)) job.reject(failure);
    for (const slot of workers) {
      slot.job?.reject(failure);
      slot.job = null;
      void slot.worker.terminate();
    }
  }
  function dispatch() {
    if (closed || failure) return;
    while (queue.length) {
      let slot = workers.find(slot => !slot.job);
      if (!slot) {
        if (workers.length >= size) return;
        const worker = new Worker(new URL('./worker.mjs', import.meta.url), { execArgv: [] });
        slot = { worker, job: null };
        workers.push(slot);
        worker.on('message', message => {
          const job = slot.job;
          slot.job = null;
          if (!job) return fail(new Error('unexpected syntax worker response'));
          try {
            onWorker?.({ threadId: worker.threadId, kind: job.task.kind });
            if (message.error) {
              const error = new Error(message.error.message);
              error.stack = message.error.stack;
              job.reject(error);
            } else job.resolve(message.result);
          } catch (error) { job.reject(error); fail(error); }
          dispatch();
        });
        worker.on('error', fail);
        worker.on('exit', code => {
          if (!closed && !failure) fail(new Error(`syntax worker exited unexpectedly: ${code}`));
        });
      }
      slot.job = queue.shift();
      try { slot.worker.postMessage(slot.job.task); }
      catch (error) { fail(error); }
    }
  }
  return {
    run(task) {
      if (closed || failure) return Promise.reject(failure ?? new Error('syntax pool is closed'));
      return new Promise((resolve, reject) => { queue.push({ task, resolve, reject }); dispatch(); });
    },
    async close() {
      closed = true;
      const error = new Error('syntax pool is closed');
      for (const job of queue.splice(0)) job.reject(error);
      for (const slot of workers) { slot.job?.reject(error); slot.job = null; }
      await Promise.all(workers.map(slot => slot.worker.terminate()));
    },
  };
}
