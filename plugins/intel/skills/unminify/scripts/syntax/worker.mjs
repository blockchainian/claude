import { parse } from '@babel/parser';
import { parentPort } from 'node:worker_threads';
import { transformSyntax } from './runner.mjs';
import { mergeExternalImports } from '../merge-imports.mjs';
import { summarizeFragmentFile } from '../fragments.mjs';

parentPort.on('message', task => {
  try {
    let result;
    if (task.kind === 'parse') {
      const started = performance.now();
      let parseable = true;
      try { parse(task.source, { sourceType: 'unambiguous', allowUndeclaredExports: true }); }
      catch (error) { if (error instanceof SyntaxError) parseable = false; else throw error; }
      result = { parseable, elapsed: performance.now() - started };
    } else if (task.kind === 'fragment') result = summarizeFragmentFile(task.path);
    else if (task.kind === 'transform') {
      const { source, steps } = transformSyntax(task.source, task.options);
      result = { source, steps: steps.map(({ name, edits }) => ({ name, edits: edits.length })) };
    } else if (task.kind === 'merge') result = mergeExternalImports(task.sources, task.options);
    else throw new Error(`unknown syntax task: ${task.kind}`);
    parentPort.postMessage({ result });
  } catch (error) {
    parentPort.postMessage({ error: { message: error.message, stack: error.stack } });
  }
});
