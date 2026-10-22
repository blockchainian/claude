import test from 'node:test';
import assert from 'node:assert/strict';
import { Worker } from 'node:worker_threads';

test('chunk guard checks cross-file and nested renames within a bounded worker heap', { timeout: 120000 }, async () => {
  const result = await new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./helpers/chunk-check-memory.mjs', import.meta.url), {
      resourceLimits: { maxOldGenerationSizeMb: 128 },
    });
    let received = false;
    const deadline = setTimeout(() => {
      void worker.terminate();
      reject(new Error('Chunk guard memory regression exceeded 110 seconds'));
    }, 110000);
    worker.once('message', message => {
      received = true;
      clearTimeout(deadline);
      resolve(message);
    });
    worker.once('error', error => {
      clearTimeout(deadline);
      reject(error);
    });
    worker.once('exit', code => {
      clearTimeout(deadline);
      if (!received) reject(new Error(`Chunk guard worker exited without a result: ${code}`));
    });
  });
  assert.equal(result.valid, null);
  assert.equal(result.capture, 'CHUNK_BINDING_CHANGED');
  assert.equal(result.changedText, 'declaration.js: STRUCTURE_OR_TEXT_CHANGED');
});
