// Ported from wakaru v1.14.0 (Apache-2.0); test inputs are independently authored.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { indexFragmentContext } from '../scripts/fragments.mjs';
import rule from '../scripts/syntax/rules/un-numeric-literal.mjs';

const run = (source, options = {}) => transformSyntax(source, { ...options, rules: [rule] }).source;

test('UnNumericLiteral normalizes finite expression literals from every radix', () => {
  assert.equal(run('consume(0x2b, 0XAB, 0b10101, 0B110, 0o71, 0O17, 073);'),
    'consume(43, 171, 21, 6, 57, 15, 59);');
});

test('UnNumericLiteral normalizes decimal fractions, exponents and separators', () => {
  assert.equal(run('consume(7E2, 4.50, .375, 23., 8_765, 0xA_B, 4e-3, 6e-8, 3e+24);'),
    'consume(700, 4.5, 0.375, 23, 8765, 171, 0.004, 6e-8, 3e+24);');
});

test('UnNumericLiteral retains raw spelling equal to Rust decimal display', () => {
  assert.equal(run('consume(0.00000008, 8000000000000000000000, 8e21, 8e-8);'),
    'consume(0.00000008, 8000000000000000000000, 8e+21, 8e-8);');
});

test('UnNumericLiteral preserves overflow spelling and nonnumber literals', () => {
  const source = 'consume(4e999, 0x' + 'f'.repeat(260) + ', 0xABn, 8_765n, "0xAB", /4e999/, `0xAB`);';
  assert.equal(run(source), source);
});

test('UnNumericLiteral normalizes underflow, signed operands and nested expressions', () => {
  assert.equal(run('consume(7e-999, -0x2b, +0o71, -0e5, delete cache[0x2b], void (0xAB));'),
    'consume(0, -43, +57, -0, delete cache[43], void (171));');
});

test('UnNumericLiteral keeps noncomputed property names but visits computed expressions', () => {
  assert.equal(run('const record = { 0x2b: 0xAB, 7e2() {}, [0o71]: 0b110 }; class Item { 0x2b = 0xAB; 7e2() {} [0o71]() { return 0b110; } }'),
    'const record = { 0x2b: 171, 7e2() {}, [57]: 6 }; class Item { 0x2b = 171; 7e2() {} [57]() { return 6; } }');
});

test('UnNumericLiteral keeps member access valid when a radix literal becomes an integer', () => {
  assert.equal(run('consume(0x2b.toString(), 23..toString(), 23. /* keep */ .toString(), 0x2b?.toString());'),
    'consume(43..toString(), 23..toString(), 23 /* keep */ .toString(), 43?.toString());');
});

test('UnNumericLiteral preserves fragment bindings, kinds and declaration layout', () => {
  const source = 'var shared = 0x2b, other = 7E2; let local = 0o71; const fixed = .375; export { missing };';
  const expected = 'var shared = 43, other = 700; let local = 57; const fixed = 0.375; export { missing };';
  const fragmentContext = indexFragmentContext(new Map([['slice.js', source], ['sibling.js', 'let Infinity, undefined; use(shared);']]));
  assert.equal(run(source, { fragment: true, fragmentContext }), expected);
  assert.equal(run(source, { fragment: true }), expected);
});

test('UnNumericLiteral needs no globals even with shadowing and dynamic scopes', () => {
  assert.equal(run('var Infinity, undefined; with (record) { consume(0x2b); } eval(code);'),
    'var Infinity, undefined; with (record) { consume(43); } eval(code);');
});

test('UnNumericLiteral preserves untouched bytes and is idempotent', () => {
  const source = '// keep\r\nconst  size = 0x2b; /* tail */\r\nconsume( size, 43, 0.375 );\r\n';
  const expected = '// keep\r\nconst  size = 43; /* tail */\r\nconsume( size, 43, 0.375 );\r\n';
  assert.equal(run(source), expected);
  assert.equal(run(expected), expected);
  assert.equal(transformSyntax(expected, { rules: [rule] }).steps[0].edits.length, 0);
});
