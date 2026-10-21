import { parentPort } from 'node:worker_threads';
import { transformSyntax } from './runner.mjs';
import { mergeExternalImports } from '../merge-imports.mjs';

parentPort.on('message', task => {
  try {
    let result;
    if (task.kind === 'transform') {
      const { source, steps } = transformSyntax(task.source, task.options);
      result = { source, steps: steps.map(({ name, edits }) => ({ name, edits: edits.length })) };
    } else if (task.kind === 'merge') result = mergeExternalImports(task.sources, task.options);
    else throw new Error(`unknown syntax task: ${task.kind}`);
    parentPort.postMessage({ result });
  } catch (error) {
    parentPort.postMessage({ error: { message: error.message, stack: error.stack } });
  }
});
