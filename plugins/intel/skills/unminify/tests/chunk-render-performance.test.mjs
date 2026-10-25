import test from 'node:test';
import assert from 'node:assert/strict';
import { analyzeChunk, renderChunkBatch, releaseChunkAnalysis } from '../scripts/chunks.mjs';

const fixture = () => new Map([['sample.js', Array.from({ length: 400 }, (_, index) =>
  `const item${index} = ${index}; console.log(item${index});`).join('\n')]]);

// Observe reads of real Babel nodes without replacing parsing or rendering behavior.
function countStarts(nodes) {
  let reads = 0;
  for (const node of new Set(nodes)) {
    const start = node.start;
    Object.defineProperty(node, 'start', { configurable: true, get() { reads++; return start; } });
  }
  return () => reads;
}

test('rendering one name does not rebuild records for unrelated declarations', t => {
  const sources = fixture();
  const analysis = analyzeChunk(sources);
  const reads = countStarts([...analysis.bindings.values()]
    .filter(record => record.name !== 'item399')
    .flatMap(record => record.declarations.map(({ binding }) => binding.identifier)));
  const result = renderChunkBatch(sources, { id: 'chunk:100000:item399', keys: ['item399'] });
  assert.match(result.context, /const v1 = 399; console.log\(v1\);/);
  t.diagnostic(`unrelated declaration start reads: ${reads()}`);
  assert.ok(reads() < 100, `unrelated declaration start reads: ${reads()}`);
});

test('rendering an excerpt visits its lexical range rather than the whole file', t => {
  const sources = fixture();
  const analysis = analyzeChunk(sources);
  const reads = countStarts(analysis.files.get('sample.js').lexical.map(path => path.node));
  const result = renderChunkBatch(sources, { id: 'chunk:100000:item399', keys: ['item399'] });
  assert.match(result.context, /const v1 = 399; console.log\(v1\);/);
  t.diagnostic(`lexical start reads: ${reads()}`);
  assert.ok(reads() < 300, `lexical start reads: ${reads()}`);
});

test('indexed rendering preserves sibling order, shadowing and placeholder collisions', () => {
  const sources = new Map([
    ['decl.js', 'const a = 1; const v1 = 2; function f(a) { return a + v1; }'],
    ['use.js', 'console.log(a, v1, f(a));'],
  ]);
  const batch = { id: 'chunk:100000:a', keys: ['a'] };
  const expected = {
    context: '// decl.js\nconst v1 = 1; const _context1 = 2; function f(a) { return a + _context1; }\n\n'
      + '// use.js\nconsole.log(v1, _context1, f(v1));\n// ...\nconsole.log(v1, _context1, f(v1));',
    placeholders: { v1: 'a' },
  };
  assert.deepEqual(renderChunkBatch(sources, batch), expected);
  releaseChunkAnalysis(sources);
  assert.deepEqual(renderChunkBatch(sources, batch), expected);
  sources.set('use.js', 'console.log(a);');
  assert.equal(renderChunkBatch(sources, batch).context,
    expected.context.slice(0, expected.context.indexOf('// use.js')) + '// use.js\nconsole.log(v1);');
});
