// Ported from wakaru v1.14.0 (Apache-2.0); independently authored inputs.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/un-typeof-strict.mjs';

const run = (source, fragment = false) => transformSyntax(source, { rules: [rule], fragment }).source;

for (const operator of ['==', '!=']) {
  for (const reverse of [false, true]) {
    test(`UnTypeofStrict upgrades ${operator}, reversed=${reverse}`, () => {
      const expression = reverse ? `"mystery" ${operator} typeof inspect()` : `typeof inspect() ${operator} "mystery"`;
      assert.equal(run(`consume(${expression});`), `consume(${expression.replace(operator, `${operator}=`)});`);
    });
  }
}

test('UnTypeofStrict retains unmatched operands, operators and explicit operand parentheses', () => {
  const source = 'consume(typeof item === "number", typeof item !== "number", typeof item < "u", typeof item == 4, typeof item == `string`, item == "string", (typeof item) == "string", typeof item == ("string"));';
  assert.equal(run(source), source);
});

test('UnTypeofStrict transforms nested comparisons, including delete operands', () => {
  assert.equal(run('consume(typeof (typeof record != "object") == "boolean", delete cache[typeof key == "symbol"]);'),
    'consume(typeof (typeof record !== "object") === "boolean", delete cache[typeof key === "symbol"]);');
});

test('UnTypeofStrict preserves comments, strings and whitespace around its operator', () => {
  assert.equal(run('// keep\r\nconsume(typeof item /* == fake */ != /* != fake */ "o\\u0062ject");\r\n'),
    '// keep\r\nconsume(typeof item /* == fake */ !== /* != fake */ "o\\u0062ject");\r\n');
  assert.equal(run('consume(typeof item // != ignored\n== "object");'), 'consume(typeof item // != ignored\n=== "object");');
});

test('UnTypeofStrict preserves fragment bindings, declaration kinds and layout', () => {
  const source = 'var shared = typeof item == "string", second; let local; const check = typeof other != "object"; export { missing };';
  assert.equal(run(source, true), 'var shared = typeof item === "string", second; let local; const check = typeof other !== "object"; export { missing };');
});

test('UnTypeofStrict introduces no globals and needs no shadowing exemption', () => {
  assert.equal(run('function check(undefined) { return typeof undefined == "undefined"; }'),
    'function check(undefined) { return typeof undefined === "undefined"; }');
});

test('UnTypeofStrict recognizes Unicode line endings in operator comments', () => {
  for (const newline of ['\u2028', '\u2029']) {
    assert.equal(run(`consume(typeof item // keep${newline}== "object");`),
      `consume(typeof item // keep${newline}=== "object");`);
  }
});
