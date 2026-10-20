// Ported from wakaru 1.14.0 (Apache-2.0); these test inputs are independently authored.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/remove-void.mjs';

const run = (source, fragment = false) => transformSyntax(source, { rules: [rule], fragment }).source;

test('RemoveVoid restores undefined for numeric literal operands', () => {
  assert.equal(run('consume(void 42, void (0xF), void ((2.5)), void 1e3);'), 'consume(undefined, undefined, undefined, undefined);');
});

test('RemoveVoid retains nonnumeric operands and all delete subtrees', () => {
  const source = 'consume(void -1, void +0, void 4n, void item, void notify(), void (1, 2), delete cache[void 0], delete (void 2));';
  assert.equal(run(source), source);
});

test('RemoveVoid skips a module if any scope binds undefined', () => {
  for (const binding of [
    'let undefined;',
    'function local(undefined) {}',
    'function local() { const { value: undefined } = record; }',
    'try { work(); } catch (undefined) {}',
    'const local = function undefined() {};',
    'const local = class undefined {};',
    'import { value as undefined } from "pkg";',
  ]) {
    const source = `consume(void 8); ${binding}`;
    assert.equal(run(source), source);
  }
});

test('RemoveVoid skips uncertain dynamic scopes and eval mentioning undefined', () => {
  for (const dynamic of ['with (record) { work(); }', 'eval(code);', 'eval("var undefined = 7");']) {
    const source = `consume(void 8); ${dynamic}`;
    assert.equal(run(source), source);
  }
});

test('RemoveVoid permits known eval that does not mention undefined', () => {
  assert.equal(run('eval("work()"); consume(void 6);'), 'eval("work()"); consume(undefined);');
});

test('RemoveVoid never introduces undefined into a fragment', () => {
  const source = 'var shared = void 0; function inspect(){ return void 9; }';
  assert.equal(run(source, true), source);
});

test('RemoveVoid preserves every byte outside replaced expressions', () => {
  const source = '// retained\r\nconst  absent = (void 3); /* untouched */\r\nconsume( absent );\r\n';
  assert.equal(run(source), '// retained\r\nconst  absent = (undefined); /* untouched */\r\nconsume( absent );\r\n');
});
