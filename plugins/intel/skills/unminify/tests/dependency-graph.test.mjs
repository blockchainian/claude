import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { buildDependencyGraph } from '../scripts/dependency-graph.mjs';

const fixture = id => readFileSync(new URL(`./fixtures/webpack/${id}.js`, import.meta.url), 'utf8');
test('real recovered webpack modules use the third parameter and exclude unselected dependencies', () => {
  const sources = new Map(['120048', '718895', '549213'].map(id => [`src/${id}.js`, fixture(id)]));
  const graph = buildDependencyGraph([...sources.keys()], { root: '/tmp', sources });
  assert.deepEqual(graph.edges, [
    { source: 'src/120048.js', target: 'src/549213.js' },
    { source: 'src/120048.js', target: 'src/718895.js' },
  ]);
  assert.equal(graph.nodes.find(node => node.id === 'src/718895.js').moduleId, 718895);
});
test('webpack require references respect lexical shadowing', () => {
  const original = fixture('549213');
  const source = original.replace('n.d(t, {', 'function shadow(n) { n(718895); }\n  n.d(t, {');
  const sources = new Map([['a.js', source], ['b.js', fixture('718895')]]);
  assert.deepEqual(buildDependencyGraph([...sources.keys()], { root: '/tmp', sources }).edges, []);
});
test('relative import and export sources preserve literal edit ranges and external links', () => {
  const source = "import value from './b.js';\nexport * from '../outside.js';\nexport { value };\n";
  const sources = new Map([['src/a.js', source], ['src/b.js', 'export default 1;']]);
  const graph = buildDependencyGraph([...sources.keys()], { root: '/tmp', sources });
  assert.deepEqual(graph.edges, [{ source: 'src/a.js', target: 'src/b.js' }]);
  assert.deepEqual(graph.nodes[0].imports.map(link => [source.slice(link.start, link.end), link.target]), [
    ["'./b.js'", 'src/b.js'], ["'../outside.js'", 'outside.js'],
  ]);
});
test('duplicate webpack module IDs fail explicitly', () => {
  const sources = new Map([['a.js', fixture('549213')], ['b.js', fixture('549213')]]);
  assert.throws(() => buildDependencyGraph([...sources.keys()], { root: '/tmp', sources }), /duplicate webpack module/i);
});
