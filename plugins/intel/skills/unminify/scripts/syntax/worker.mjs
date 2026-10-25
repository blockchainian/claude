import { parse } from '@babel/parser';
import { withTimings, timed } from '../timing.mjs';
import { parentPort } from 'node:worker_threads';
import { transformSyntax } from './runner.mjs';
import { mergeExternalImports } from '../merge-imports.mjs';
import { summarizeFragmentFile } from '../fragments.mjs';
import { summarizeImportFile, resolveNames, applyRenames } from '../rename.mjs';
import { checkRewrite } from '../checks.mjs';
import { analyzeImportChunkFile, applyImportChunkFile, checkImportChunkFile } from '../import-chunk-file.mjs';

function runTask(task) {
  let result;
  if (task.kind === 'parse') {
    const started = performance.now();
    let parseable = true;
    try { parse(task.source, { sourceType: 'unambiguous', allowUndeclaredExports: true }); }
    catch (error) { if (error instanceof SyntaxError) parseable = false; else throw error; }
    result = { parseable, elapsed: performance.now() - started };
  } else if (task.kind === 'fragment') result = summarizeFragmentFile(task.path);
  else if (task.kind === 'import-analyze') result = summarizeImportFile(task.source, task.options);
  else if (task.kind === 'import-resolve') result = timed('resolveNames', () => resolveNames(task.source, task.proposals, task.options));
  else if (task.kind === 'import-apply') result = timed('applyRenames', () => applyRenames(task.source, task.names));
  else if (task.kind === 'import-check') result = timed('checkRewrite', () => checkRewrite(task.before, task.after, task.options));
  else if (task.kind === 'import-chunk-analyze') result = analyzeImportChunkFile(task.source);
  else if (task.kind === 'import-chunk-apply') result = applyImportChunkFile(task.source, new Map(task.names), task.record);
  else if (task.kind === 'import-chunk-check') result = checkImportChunkFile(task);
  else if (task.kind === 'transform') {
    const { source, steps } = transformSyntax(task.source, task.options);
    result = { source, steps: steps.map(({ name, edits }) => ({ name, edits: edits.length })) };
  } else if (task.kind === 'merge') result = mergeExternalImports(task.sources, task.options);
  else throw new Error(`unknown syntax task: ${task.kind}`);
  return result;
}

parentPort.on('message', task => {
  const timings = {};
  try {
    const result = withTimings(timings, () => runTask(task));
    parentPort.postMessage({ result, timings });
  } catch (error) {
    parentPort.postMessage({ error: { message: error.message, stack: error.stack }, timings });
  }
});
