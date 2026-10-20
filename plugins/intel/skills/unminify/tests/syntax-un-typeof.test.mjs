// Ported from wakaru v1.14.0 (Apache-2.0); independently authored inputs.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/un-typeof.mjs';

const run = (source, fragment = false) => transformSyntax(source, { rules: [rule], fragment }).source;

for (const [expression, expected] of [
  ['typeof lookup() > "u"', 'typeof lookup() === "undefined"'],
  ['typeof lookup() < "u"', 'typeof lookup() !== "undefined"'],
  ['"u" < typeof lookup()', 'typeof lookup() === "undefined"'],
  ['"u" > typeof lookup()', 'typeof lookup() !== "undefined"'],
]) {
  test(`UnTypeof restores ${expression}`, () => {
    assert.equal(run(`consume(${expression});`), `consume(${expected});`);
  });
}

test('UnTypeof retains other operators, operands and explicit operand parentheses byte for byte', () => {
  const source = 'consume(typeof item <= "u", typeof item >= "u", typeof item == "u", typeof item != "u", typeof item === "u", typeof item !== "u", typeof item > "undefined", typeof item < "U", typeof item < `u`, item > "u", "u" < item, "u" + typeof item, (typeof item) > "u", typeof item > ("u"));';
  assert.equal(run(source), source);
});

test('UnTypeof recognizes escaped u and preserves all surrounding bytes', () => {
  assert.equal(run('// keep\r\nconsume( typeof item /* > fake */ > /* keep */ "\\x75" );\r\n'),
    '// keep\r\nconsume( typeof item /* > fake */ === /* keep */ "undefined" );\r\n');
});

test('UnTypeof retains comments when reversing operands', () => {
  assert.equal(run('consume("u" /* < keep */ < /* tail */ typeof item);'),
    'consume(typeof item /* < keep */ === /* tail */ "undefined");');
});

test('UnTypeof composes nested edits without replacing the same interval twice', () => {
  assert.equal(run('consume("u" < typeof (typeof item > "u"), typeof ("u" > typeof key) < "u");'),
    'consume(typeof (typeof item === "undefined") === "undefined", typeof (typeof key !== "undefined") !== "undefined");');
});

test('UnTypeof keeps the comparison grouped in binary parent expressions', () => {
  assert.equal(run('consume(typeof item > "u" < limit, "u" > typeof item === flag);'),
    'consume((typeof item === "undefined") < limit, (typeof item !== "undefined") === flag);');
});

test('UnTypeof also visits delete operands', () => {
  assert.equal(run('delete cache[typeof key < "u"];'), 'delete cache[typeof key !== "undefined"];');
});

test('UnTypeof preserves fragment binding kinds and declaration layout', () => {
  assert.equal(run('var shared = typeof item > "u", second; let local; const check = "u" > typeof other; export { missing };', true),
    'var shared = typeof item === "undefined", second; let local; const check = typeof other !== "undefined"; export { missing };');
});

test('UnTypeof emits a string, so shadowed undefined and dynamic scopes are safe', () => {
  assert.equal(run('function check(undefined) { eval(code); return typeof undefined > "u"; }'),
    'function check(undefined) { eval(code); return typeof undefined === "undefined"; }');
});
