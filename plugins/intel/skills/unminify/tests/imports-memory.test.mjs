import test from 'node:test';
import assert from 'node:assert/strict';
import { followImports } from '../scripts/rename.mjs';

test('followImports releases syntax and scope data before visiting another file', () => {
  const files = new Map();
  for (let index = 0; index < 200; index++) {
    const declarations = Array.from({ length: 100 }, (_, binding) => `const value${binding}=${binding};`).join('');
    files.set(`src/module${index}.js`, `${declarations} export {value0 as entry};`);
  }
  files.set('src/main.js', 'import {entry as x} from "./module199.js"; console.log(x);');
  let summaries = 0;
  const changes = followImports(files, (file, task) => {
    const result = task();
    // This boundary ends the file's parse phase. Only compact module metadata
    // may escape it; keeping analysis here retains its entire Babel graph.
    if (result && typeof result === 'object' && !(result instanceof Map)) {
      assert.equal(result.ast === undefined, true, `${file} retained its AST`);
      assert.equal(result.program === undefined, true, `${file} retained its program scope`);
      assert.equal(result.analysis === undefined, true, `${file} retained its analysis`);
      for (const name of result.bindings.values()) assert.equal(typeof name, 'string');
      summaries++;
    }
    return result;
  });
  assert.equal(summaries, files.size);
  assert.equal(changes.size, 1);
  assert.equal(changes.get('src/main.js'), 'import {entry as value0} from "./module199.js"; console.log(value0);');
  const resumed = new Map(files);
  for (const [file, source] of changes) resumed.set(file, source);
  assert.equal(followImports(resumed).size, 0);
});
