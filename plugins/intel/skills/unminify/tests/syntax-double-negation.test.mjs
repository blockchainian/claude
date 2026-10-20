// Ported from wakaru v1.14.0 (Apache-2.0); examples independently authored.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/double-negation.mjs';

const run = (source, options = {}) => transformSyntax(source, { ...options, rules: [rule] }).source;

for (const [label, source, expected] of [
  ['if', 'if (!!permission) enter(); else refuse();', 'if (permission) enter(); else refuse();'],
  ['while', 'while (!!queue.length) drain();', 'while (queue.length) drain();'],
  ['do while', 'do tick(); while (!!next());', 'do tick(); while (next());'],
  ['for', 'for (let n = 4; !!available(n); n--) visit(n);', 'for (let n = 4; available(n); n--) visit(n);'],
  ['conditional', 'const style = !!settings.dark ? night : day;', 'const style = settings.dark ? night : day;'],
]) {
  test(`UnDoubleNegation strips one pair in ${label} tests`, () => {
    assert.equal(run(source), expected);
  });
}

test('UnDoubleNegation leaves missing for tests and for-in/of expressions untouched', () => {
  const source = 'for (;;) break; for (const key in !!record) visit(key); for (const entry of !!items) visit(entry);';
  assert.equal(run(source), source);
});

test('UnDoubleNegation requires two consecutive logical not operators', () => {
  const source = 'if (enabled) act(); while (!closed) wait(); do act(); while (~!mask); for (; +!pending;) break; const a = !~bits ? on : off;';
  assert.equal(run(source), source);
});

test('UnDoubleNegation keeps value-producing double negations byte for byte', () => {
  const source = 'const flag = !!item; output = !!flag; consume(!!item); function check() { return !!item; } const arrow = () => !!item; !!standalone;';
  assert.equal(run(source), source);
});

test('UnDoubleNegation keeps nested logical and sequence operands untouched', () => {
  const source = 'if (!!left && !!right) act(); while (first || !!second) wait(); for (; (touch(), !!last);) break; const v = !(!inner && flag) ? !!yes : !!no;';
  assert.equal(run(source), source);
});

test('UnDoubleNegation respects parentheses interrupting the matched pair', () => {
  const source = 'if ((!!ready)) act(); while (!(!waiting)) wait(); do tick(); while (((!!again))); for (; (!(!available));) break; const v = (!!selected) ? a : b;';
  assert.equal(run(source), source);
});

test('UnDoubleNegation retains parentheses around the surviving operand', () => {
  const source = 'if (!!(first || second)) act(); const v = !!(pick = next()) ? yes : no; while (!!(touch(), pending)) wait();';
  assert.equal(run(source), 'if ((first || second)) act(); const v = (pick = next()) ? yes : no; while ((touch(), pending)) wait();');
});

test('UnDoubleNegation removes exactly one pair per test in each pass', () => {
  assert.equal(run('if (!!!!ready) act(); const v = !!!pending ? a : b;'), 'if (!!ready) act(); const v = !pending ? a : b;');
});

test('UnDoubleNegation transforms nested conditions without overlapping edits', () => {
  const source = 'if (!!(!!selector ? !!left : !!right)) { while (!!running) { if (!!done) break; } }';
  assert.equal(run(source), 'if ((selector ? !!left : !!right)) { while (running) { if (done) break; } }');
});

test('UnDoubleNegation visits conditions in every expression subtree', () => {
  const source = 'delete cache[!!selector ? a : b]; consume(() => { if (!!ready) run(); });';
  assert.equal(run(source), 'delete cache[selector ? a : b]; consume(() => { if (ready) run(); });');
});

test('UnDoubleNegation preserves comments, spacing and line endings outside operator bytes', () => {
  const source = '// stay\r\nif (! /*outer*/ ! /*inner*/ enabled) { /* body */ act(); }\r\n';
  assert.equal(run(source), '// stay\r\nif ( /*outer*/  /*inner*/ enabled) { /* body */ act(); }\r\n');
});

test('UnDoubleNegation keeps for initializers, updates and conditional branches', () => {
  const source = 'for (let bit = !!start; !!keep; bit = !!next) use(bit); const v = !!test ? !!a : !!b;';
  assert.equal(run(source), 'for (let bit = !!start; keep; bit = !!next) use(bit); const v = test ? !!a : !!b;');
});

test('UnDoubleNegation preserves fragment bindings, declaration kinds and layout', () => {
  const source = 'var shared = !!input, spare; let retained; const chosen = !!ready ? first : second; if (!!shared) consume(shared); function local() { var own = !!value; if (!!own) return own; } export { shared, external };';
  assert.equal(run(source, { fragment: true }), 'var shared = !!input, spare; let retained; const chosen = ready ? first : second; if (shared) consume(shared); function local() { var own = !!value; if (own) return own; } export { shared, external };');
});

test('UnDoubleNegation introduces no globals even with bindings or dynamic scopes', () => {
  const source = 'var Boolean = custom; function check(undefined, Infinity) { if (!!input) return input; } with (scope) { if (!!value) act(); } eval(code);';
  assert.equal(run(source, { fragment: true }), 'var Boolean = custom; function check(undefined, Infinity) { if (input) return input; } with (scope) { if (value) act(); } eval(code);');
});

test('UnDoubleNegation keeps object tests valid at statement and arrow-body starts', () => {
  const source = '!!{} ? accept() : reject(); const pick = () => !!{ active: true } ? yes : no;';
  assert.equal(run(source), '({}) ? accept() : reject(); const pick = () => ({ active: true }) ? yes : no;');
});

test('UnDoubleNegation keeps function and class tests in expression grammar', () => {
  const source = '!!function named() {} ? accept() : reject(); !!class {} ? yes() : no();';
  assert.equal(run(source), '(function named() {}) ? accept() : reject(); (class {}) ? yes() : no();');
});

test('UnDoubleNegation keeps conditional tests valid after export default', () => {
  const source = 'export default !!class Choice {} ? first : second;';
  assert.equal(run(source), 'export default (class Choice {}) ? first : second;');
});

test('UnDoubleNegation protects expression starts through calls and member access', () => {
  const source = '!!function factory() {}() ? a() : b(); !!{}.active ? yes() : no(); !!class {}.name ? x() : y();';
  assert.equal(run(source), '(function factory() {}()) ? a() : b(); ({}.active) ? yes() : no(); (class {}.name) ? x() : y();');
});

test('UnDoubleNegation protects let bracket access from declaration grammar', () => {
  const source = '!!let[index]() ? accept() : reject();';
  assert.equal(run(source), '(let[index]()) ? accept() : reject();');
});

test('UnDoubleNegation protects optional chains and tagged template starts', () => {
  const source = '!!{}?.active ? yes() : no(); !!function probe() {}?.() ? a() : b(); !!function tag() {}`text` ? x() : y();';
  assert.equal(run(source), '({}?.active) ? yes() : no(); (function probe() {}?.()) ? a() : b(); (function tag() {}`text`) ? x() : y();');
});

test('UnDoubleNegation protects postfix starts and leaves ordinary call starts bare', () => {
  const source = '!!let[index]++ ? yes() : no(); !!sensor.read()?.status ? a() : b(); !!++counter ? x() : y();';
  assert.equal(run(source), '(let[index]++) ? yes() : no(); sensor.read()?.status ? a() : b(); ++counter ? x() : y();');
});
