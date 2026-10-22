import { parentPort, workerData, threadId } from 'node:worker_threads';
import * as select from './select.mjs';
import * as rename from './rename.mjs';
import * as checks from './checks.mjs';
import * as syntax from './syntax/stage.mjs';
import { createChunkRun } from './chunk-run.mjs';
import { withTimings } from './timing.mjs';

const pending = new Map();
let nextRequest = 0, nextPhase = 0;
parentPort.on('message', message => {
  const task = pending.get(message.id);
  if (!task) return;
  pending.delete(message.id);
  if (message.error) task.reject(Object.assign(new Error(message.error.message), message.error));
  else task.resolve(message.result);
});
const namer = { name(payload, metadata) {
  const id = ++nextRequest;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    parentPort.postMessage({ type: 'name', id, payload, metadata });
  });
} };
const emit = event => parentPort.postMessage({ type: 'event', event });
const phase = (name, file) => {
  const id = ++nextPhase;
  parentPort.postMessage({ type: 'phase-start', id, name, file });
  return () => parentPort.postMessage({ type: 'phase-end', id });
};
const summary = { completed: 0, skipped: 0, rejected: 0, failedBatches: 0 };
const sources = new Map(), cpu_ms = {};
try {
  const { root, settings, selected, fragmentContext, job } = workerData;
  const driver = await createChunkRun(root, settings, undefined, { recover: false });
  driver.select(selected, new Set());
  await withTimings(cpu_ms, () => driver.process(job, {
    api: { ...select, ...rename, ...checks, ...syntax }, namer, withSlot: task => task(),
    emit, phase, summary, sources, fragmentContext,
  }));
  parentPort.postMessage({ type: 'done', chunk: job.id, threadId, summary, sources, cpu_ms });
} catch (error) {
  parentPort.postMessage({ type: 'failed', error: { message: error.message, stack: error.stack, code: error.code },
    chunk: workerData.job.id, threadId, summary, sources, cpu_ms });
}
