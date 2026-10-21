// Ported from wakaru v1.14.0 (Apache-2.0); independently authored inputs.
import assert from 'node:assert/strict';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { parseSyntax } from '../scripts/syntax/guards.mjs';
import rule from '../scripts/syntax/rules/simplify-sequence.mjs';

const run = (source, fragment = false) => transformSyntax(source, { rules: [rule], fragment }).source;
function shape(source) {
  return JSON.stringify(parseSyntax(source), (key, value) =>
    ['start', 'end', 'loc', 'extra', 'leadingComments', 'trailingComments', 'innerComments', 'comments'].includes(key) ? undefined
      : value?.type === 'ParenthesizedExpression' ? value.expression : value);
}
const equal = (source, expected, fragment = false) => assert.equal(shape(run(source, fragment)), shape(expected));

test('splits expression statements and preserves inert operands', () => {
  equal('prepare(), 73, finish(); (left(), right());', 'prepare(); 73; finish(); left(); right();');
  equal('("checkpoint", finish());', '"checkpoint"; finish();');
});
test('splits return and throw operands through nested parentheses', () => {
  equal('function inspect(){ return ((trace(), read(), result)); } try { throw (record(), fault); } catch(e) {}',
    'function inspect(){ trace(); read(); return result; } try { record(); throw fault; } catch(e) {}');
});
test('normalizes both if branches and nested else-if conditions', () => {
  equal('if (log(), ready) first(), second(); else if (probe(), retry) third(), fourth();',
    'log(); if (ready) { first(); second(); } else { probe(); if (retry) { third(); fourth(); } }');
});
test('splits switch discriminants and case statement lists', () => {
  equal('switch (mark(), key) { case 5: a(), b(); break; default: c(), d(); }',
    'mark(); switch (key) { case 5: a(); b(); break; default: c(); d(); }');
});
test('splits variable kinds, patterns and initializer prefixes in order', () => {
  equal('var spare, value=(open(), close()); let {x}=(scan(), record), y=2; const z=(tick(), ()=>7);',
    'var spare; open(); var value=close(); scan(); let {x}=record; let y=2; tick(); const z=()=>7;');
});
test('extracts assignment-member objects and preserves both operators', () => {
  equal('(holder = acquire())[field()] += delta(); (count += next()).total = 9;',
    'holder = acquire(); holder[field()] += delta(); count += next(); count.total = 9;');
});
test('with anywhere prevents assignment-member re-reads', () => {
  const source = '(holder = acquire()).total = 9; function inspect(){ with (scope) { work(); } }';
  assert.equal(run(source), source);
});
test('rejects assignment-member objects without a simple identifier target', () => {
  const source = '(box.slot = acquire()).total = 9; ([x] = values).total = 4;';
  assert.equal(run(source), source);
});
test('moves direct calls out of for headers and retains final assignments', () => {
  equal('for (setup(); ready; tick()) {} for ((seed(), index=3); ready;) {} for ((seed(), test()); ready;) {}',
    'setup(); for (; ready; tick()) {} seed(); for (index=3; ready;) {} seed(); test(); for (; ready;) {}');
});
test('retains unsupported standalone for initializers and iteration expressions', () => {
  const source = 'for (obj.setup(); ready; (a(), b())) {} for (counter++; ready;) {} for (index=0; ready;) {} for (flag; ready;) {}';
  assert.equal(run(source), source);
});
test('flushes earlier var initializers before later prefixes', () => {
  equal('for (var a=first(), b=(middle(), second()), c=(later(), third()); ready;) {}',
    'var a=first(); middle(); var b=second(); later(); for (var c=third(); ready;) {}');
});
test('extracts first independent lexical prefix but keeps later ones', () => {
  equal('for (let a=(trace(), 1), b=(notify(), 2); ready;) {}',
    'trace(); for (let a=1, b=(notify(), 2); ready;) {}');
});
test('keeps lexical TDZ references, shadowed references, eval and string prefixes safe', () => {
  for (const source of [
    'for (let a=(a, 1); ready;) {}',
    'for (let a=(b, 1), b=2; ready;) {}',
    'for (let a=((b)=>0, 1), b=2; ready;) {}',
    'for (const {a}=(a, record); ready;) {}',
    'for (let [a]=(a, record); ready;) {}',
    'for (let {slot:a=1,...rest}=(rest, record); ready;) {}',
    'for (let [a=1,...rest]=(rest, record); ready;) {}',
    'for (let a=(eval("a"), 1); ready;) {}',
    'for (let a=(eval(), 1); ready;) {}',
    'for (let a=(()=>{with(scope){}}, 1); ready;) {}',
    'for (var a=("use strict", 1); ready;) {}',
    'for (let a=("marker", 1); ready;) {}',
  ]) assert.equal(run(source), source);
  equal('for (let a=((a)=>a, 1); ready;) {}', '(a)=>a; for (let a=1; ready;) {}');
  equal('for (let a=((0, eval)("work()"), 1); ready;) {}', '(0, eval)("work()"); for (let a=1; ready;) {}');
});
test('extracts for-in and for-of iterables only without lexical headers', () => {
  equal('for (var key in (mark(), table)) {} for (item of (log(), list)) {} async function f(){ for await (var x of (open(), stream)) {} }',
    'mark(); for (var key in table) {} log(); for (item of list) {} async function f(){ open(); for await (var x of stream) {} }');
  for (const source of ['for (let x of (log(), list)) {}', 'for (const x in (log(), table)) {}']) assert.equal(run(source), source);
});
test('only splits standalone statements and if branches', () => {
  const source = 'while (ready) a(), b(); label: c(), d(); for (;;) e(), f(); consume((g(), h()));';
  assert.equal(run(source), source);
  equal('while (ready) { a(), b(); }', 'while (ready) { a(); b(); }');
});
test('handles yields without reordering their argument sequence', () => {
  equal('function* feed(){ yield (log(), item); yield* (log(), items); (yield item), finish(); }',
    'function* feed(){ yield (log(), item); yield* (log(), items); (yield item); finish(); }');
});
test('combines overlapping nested rewrites into a single enclosing edit', () => {
  equal('let fn=(mark(), function(){ return (log(), value); }), end=(done(), 1);',
    'mark(); let fn=function(){ log(); return value; }; done(); let end=1;');
});
test('preserves export declaration boundaries', () => {
  const source = 'export const a=(mark(), value), b=2;';
  assert.equal(run(source), source);
});
test('preserves resource declaration grouping and initializer sequences', () => {
  const source = 'function inspect(){ using a=(open(), resource), b=acquire(); }';
  assert.equal(run(source), source);
});
test('preserves in-expression grammar when extracting for initializers', () => {
  equal('for (var item=(trace(), key in table); ready;) {} for ((trace(), item=key in table); ready;) {}',
    'trace(); for (var item=(key in table); ready;) {} trace(); for ((item=key in table); ready;) {}');
});
test('keeps expression-only syntax valid when it becomes a statement', () => {
  equal('(function(){}(), {answer: 7}.answer, class {}.name, done());',
    '(function(){}()); ({answer:7}.answer); (class {}.name); done();');
});
test('fragments allow splitting declarations and expressions in evaluation order', () => {
  for (const [source, expected] of [
    ['var a=(mark(), value), b=2;', 'mark();\nvar a = value;\nvar b=2;'],
    ['let a=(mark(), value);', 'mark();\nlet a = value;'],
    ['for (var a=(mark(), value); ready;) {}', 'mark();\nfor (var a = (value); ready;) {}'],
    ['mark(), done(); var a=2;', 'mark();\ndone(); var a=2;'],
    ['if (ready) { mark(), done(); var shared=2; }', 'if (ready) { mark();\ndone(); var shared=2; }'],
    ['if (mark(), ready) var shared=2;', 'mark();\nif (ready) var shared=2;'],
  ]) assert.equal(run(source, true), expected);
});
test('fragment declaration splitting preserves names, kinds and evaluation order', () => {
  equal('var first=(mark(), value), second=next(); let third=(done(), last); const fourth=finish();',
    'mark(); var first=value; var second=next(); done(); let third=last; const fourth=finish();', true);
  const source = 'var events=[]; function mark(s){events.push(s);return s;} var first=(mark("prefix"),mark("first")), second=mark("second"); let third=(mark("middle"),mark("third")); const fourth=mark("fourth"); JSON.stringify(events);';
  assert.equal(runInNewContext(run(source, true)), '["prefix","first","second","middle","third","fourth"]');
});
test('fragments still simplify complete function scopes and trailing expressions', () => {
  equal('var shared=2; function inspect(){ let a=(mark(), value), b=2; return (log(), a); } done(), finish();',
    'var shared=2; function inspect(){ mark(); let a=value; let b=2; log(); return a; } done(); finish();', true);
  equal('var fn=(mark(), function(){return (log(), value);}), sibling=2;',
    'mark(); var fn=function(){log(); return value;}; var sibling=2;', true);
});
test('detects unbound-export fragments without an explicit fragment option', () => {
  const source = 'var shared=(mark(), value), sibling=2; export { missing };';
  assert.equal(run(source), 'mark();\nvar shared = value;\nvar sibling=2; export { missing };');
});
test('preserves actual initializer evaluation order', () => {
  const source = 'const events=[]; function mark(s){events.push(s);return 1;} for(var a=mark("first"), b=(mark("prefix"),mark("middle")), c=(mark("later"),mark("last"));false;){} JSON.stringify(events);';
  const expected = '["first","prefix","middle","later","last"]';
  assert.equal(runInNewContext(source), expected);
  assert.equal(runInNewContext(run(source)), expected);
});
test('preserves bytes outside replacement intervals and unchanged text', () => {
  assert.equal(run('// keep\r\n  first(), second(); /* tail */\r\n'), '// keep\r\n  first();\r\n  second(); /* tail */\r\n');
  const source = '// exact\r\nfunction  f ( ) { return  value; } /* end */\r\n';
  assert.equal(run(source), source);
  for (const unchanged of ['((start(), finish()));', '(value);', 'function inspect(){return;}', 'throw fault;', 'switch(key){}']) assert.equal(run(unchanged), unchanged);
});
