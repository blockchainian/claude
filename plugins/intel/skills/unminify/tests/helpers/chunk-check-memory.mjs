import { parentPort } from 'node:worker_threads';
import { checkChunkRewrite } from '../../scripts/chunks.mjs';

const before = new Map([['declaration.js', 'const a = 7;']]);
const after = new Map([['declaration.js', 'const amount = 7;']]);
for (let file = 0; file < 48; file++) {
  const original = [];
  const renamed = [];
  for (let index = 0; index < 160; index++) {
    const name = `read${file}_${index}`;
    original.push(`function ${name}(x) { return a + x; }`);
    // Reusing the program binding's original name verifies that the guard
    // restores nested bindings before inverting the shared program rename.
    renamed.push(`function ${name}(a) { return amount + a; }`);
  }
  before.set(`reader${file}.js`, original.join('\n'));
  after.set(`reader${file}.js`, renamed.join('\n'));
}
const valid = checkChunkRewrite(before, after);
const captured = new Map(after);
captured.set('reader0.js', captured.get('reader0.js').replace('return amount + a;', 'return a + a;'));
const capture = checkChunkRewrite(before, captured);
const changed = new Map(after);
changed.set('declaration.js', 'const amount = 8;');
const changedText = checkChunkRewrite(before, changed);
parentPort.postMessage({ valid, capture, changedText });
