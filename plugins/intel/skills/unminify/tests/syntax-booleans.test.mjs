// Ported from wakaru 1.14.0 (Apache-2.0); these test inputs are independently authored.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/unminify-booleans.mjs';

const run = (source, fragment = false) => transformSyntax(source, { rules: [rule], fragment }).source;

test('UnminifyBooleans restores numeric zero and one in complete files', () => {
  const source = 'const flags = [!0, !1, !0x0, !0b1, !0.0, !1e0];';
  assert.equal(run(source), 'const flags = [true, false, true, false, true, false];');
});

test('UnminifyBooleans leaves other values and parenthesized operands untouched', () => {
  const source = 'consume(!2, !-0, !-1, !0n, !"", !null, !value, !(0), !(1));';
  assert.equal(run(source), source);
});

test('UnminifyBooleans visits nested expressions without overlapping changes', () => {
  assert.equal(run('consume(!!0, !!!1, delete store[!0]);'), 'consume(!true, !!false, delete store[true]);');
});

test('UnminifyBooleans preserves fragment declarations and every untouched byte', () => {
  const source = '// saved header\r\nvar shared = !0, spare=!1;\r\n/* keep */ function read( ) { return !0; }\r\n';
  assert.equal(run(source, true), '// saved header\r\nvar shared = true, spare=false;\r\n/* keep */ function read( ) { return true; }\r\n');
});
