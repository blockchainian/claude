import test from 'node:test';
import assert from 'node:assert/strict';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { parseSyntax } from '../scripts/syntax/guards.mjs';
import { checkSyntaxRewrite } from '../scripts/syntax/checks.mjs';
import { indexFragmentContext } from '../scripts/fragments.mjs';

function query(source, name, options = {}) {
  const results = [];
  const rule = { name: 'ReadIntrinsic', visitor(context) { return {
    ReferencedIdentifier(path) {
      if (path.node.name === name) results.push(context.isIntrinsicGlobal(path, name));
    },
  }; } };
  transformSyntax(source, { ...options, rules: [rule] });
  return results;
}

test('intrinsic reads use the reference scope without synthesis authorization', () => {
  for (const name of ['undefined', 'NaN', 'Infinity']) {
    assert.deepEqual(query(`use(${name}); function sibling(${name}) { use(${name}); }`, name), [true, false]);
    assert.deepEqual(query(`use(${name}); let ${name};`, name), [false]);
    assert.deepEqual(query(`function outer(${name}) { return () => ${name}; }`, name), [false]);
  }
});

test('intrinsic reads require dynamic and cross-file evidence', () => {
  for (const prefix of ['with (record) {}', 'eval(code);', 'eval("var Infinity");']) {
    assert.deepEqual(query(`${prefix} use(Infinity);`, 'Infinity'), [false]);
  }
  assert.deepEqual(query('eval("inspect()"); use(Infinity);', 'Infinity'), [true]);
  const source = 'use(Infinity);';
  assert.deepEqual(query(source, 'Infinity', { fragment: true }), [false]);
  for (const shadowed of [false, true]) {
    const fragmentContext = indexFragmentContext([['piece.js', source], ['sibling.js', shadowed ? 'var Infinity;' : 'var other;']]);
    assert.deepEqual(query(source, 'Infinity', { fragment: true, fragmentContext }), [!shadowed]);
  }
});

test('a read proof neither permits synthesis nor relaxes its module-wide shadowing guard', () => {
  const rule = { name: 'ReadThenIntroduce', globals: ['Infinity'], visitor(context) { return {
    NumericLiteral(path) {
      assert.equal(context.isIntrinsicGlobal(path, 'Infinity'), true);
      context.replace(path, 'Infinity');
    },
  }; } };
  assert.throws(() => transformSyntax('use(9);', { rules: [rule] }), /NEW_GLOBALS Infinity/);
  const synthesis = { name: 'Synthesize', globals: ['Infinity'], visitor(context) { return {
    NumericLiteral(path) { assert.equal(context.canIntroduceGlobal(path, 'Infinity'), false); },
  }; } };
  transformSyntax('use(9); function sibling(Infinity) {}', { rules: [synthesis] });
});

test('syntax parsing, visitors and independent reparsing accept JSX', () => {
  const source = 'const view = <Panel active={!0}>{label}</Panel>;';
  assert.equal(parseSyntax(source).program.body[0].declarations[0].init.type, 'JSXElement');
  const rule = { name: 'JsxBoolean', visitor({ replace }) { return {
    UnaryExpression(path) { replace(path, 'true'); },
  }; } };
  const after = 'const view = <Panel active={true}>{label}</Panel>;';
  assert.equal(transformSyntax(source, { rules: [rule], fragment: true }).source, after);
  assert.equal(checkSyntaxRewrite(source, after, { fragment: true }), null);
  assert.match(checkSyntaxRewrite(source, after.replace('{label}', '{missing}')), /NEW_GLOBALS missing/);
  assert.match(checkSyntaxRewrite(source, after.replaceAll('Panel', 'MissingPanel')), /NEW_GLOBALS MissingPanel/);
});
