// Ported from wakaru v1.14.0 (Apache-2.0); independently authored test inputs.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { indexFragmentContext } from '../scripts/fragments.mjs';
import rule from '../scripts/syntax/rules/un-infinity.mjs';

const run = (source, options = {}) => transformSyntax(source, { rules: [rule], ...options }).source;

test('UnInfinity restores both signs across numeric spellings', () => {
  assert.equal(run('consume(1 / 0, -1 / 0, 0x1 / 0b0, -1e0 / 0.0);'), 'consume(Infinity, -Infinity, Infinity, -Infinity);');
});

test('UnInfinity retains all other numerator, denominator and operator forms exactly', () => {
  const source = 'consume(2 / 0, 0 / 0, 1 / -0, 1 / +0, +1 / 0, 1n / 0n, -1n / 0, 1 / 2, value / 0, 1 / value, 1 % 0, 1 * 0, -(2 / 0), (1) / 0, 1 / (0), -(1) / 0, (-1) / 0);\r\n';
  assert.equal(run(source), source);
});

test('UnInfinity checks every scope for an Infinity binding', () => {
  for (const binding of ['var Infinity;', 'let Infinity;', 'const Infinity = limit;', 'function Infinity() {}', 'class Infinity {}', 'function local(Infinity) {}', 'function local() { const { limit: Infinity } = settings; }', 'try {} catch (Infinity) {}', 'const local = function Infinity() {};', 'const local = class Infinity {};', 'import { ceiling as Infinity } from "pkg";']) {
    const source = `consume(1 / 0, -1 / 0); ${binding}`;
    assert.equal(run(source), source);
  }
});

test('UnInfinity blocks with, unknown eval and eval mentioning Infinity', () => {
  for (const dynamic of ['with (record) {}', 'eval(code);', 'eval(...codes);', 'eval("var Infinity = 9");', '(eval)("Infinity");', 'eval("quire".replace(/x/, "re"));']) {
    const source = `${dynamic} consume(1 / 0, -1 / 0);`;
    assert.equal(run(source), source);
  }
});

test('UnInfinity permits static eval without the name and indirect eval', () => {
  for (const dynamic of ['eval();', 'eval("work()");', 'eval("InfinityLimit");', 'eval("$Infinity");', 'eval("quire".replace(/^/, "re"));', '(0, eval)(code);', 'eval?.(code);']) {
    assert.equal(run(`${dynamic} consume(1 / 0);`), `${dynamic} consume(Infinity);`);
  }
});

test('UnInfinity uses cross-file context for fragments and preserves declaration layout', () => {
  const source = 'var ceiling = 1 / 0, floor = -1 / 0; let third; const fourth = 1 / 0; export { missing };';
  assert.equal(run(source, { fragment: true }), source);
  for (const declaration of ['var Infinity;', 'import { upper as Infinity } from "pkg";']) {
    const fragmentContext = indexFragmentContext([['piece.js', source], ['binding.js', declaration]]);
    assert.equal(run(source, { fragment: true, fragmentContext }), source);
  }
  const fragmentContext = indexFragmentContext([['piece.js', source], ['binding.js', 'let unrelated;']]);
  assert.equal(run(source, { fragment: true, fragmentContext }), 'var ceiling = Infinity, floor = -Infinity; let third; const fourth = Infinity; export { missing };');
});

test('UnInfinity composes nested edits without overlapping and traverses delete operands', () => {
  assert.equal(run('consume((1 / 0) / 0, -(1 / 0), 1 / (1 / 0), delete cache[1 / 0]);'), 'consume((Infinity) / 0, -(Infinity), 1 / (Infinity), delete cache[Infinity]);');
});

test('UnInfinity preserves source outside replacements and spaces compact keyword operands', () => {
  assert.equal(run('// unchanged\r\nconst  ceiling = (1 /* ratio */ / 0);\r\nfunction high(){return-1/0;}'), '// unchanged\r\nconst  ceiling = (Infinity);\r\nfunction high(){return-Infinity;}');
  assert.equal(run('function high(){return 1/0;}'), 'function high(){return Infinity;}');
});
