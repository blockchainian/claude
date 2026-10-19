import assert from 'node:assert/strict';
import test from 'node:test';
import { checkRewrite } from '../scripts/checks.mjs';

function passes(before, after = before) {
  assert.equal(checkRewrite(before, after), null);
}
function rejects(before, after, reason) {
  const result = checkRewrite(before, after);
  assert.equal(typeof result, 'string');
  assert.match(result, reason);
  assert.ok(!result.includes('\n'));
}

test('requires Node syntax, including syntax Babel alone accepts', () => {
  rejects('let e = 1;', 'let = ;', /parse/i);
  rejects('const e = 1;', 'const e = <div />;', /parse/i);
  passes('let e = 1;', 'let count = 1;');
});

test('keeps exported names, including destructuring and reexports', () => {
  rejects('export const e = 1;', 'export const count = 1;', /interface/i);
  rejects('export const { x: e } = source;', 'export const { x: count } = source;', /interface/i);
  rejects('export { x } from "a";', 'export { y } from "a";', /interface/i);
  passes('const e = 1; export { e };', 'const count = 1; export { count as e };');
  passes('export default function e() { return e; }', 'export default function count() { return count; }');
});

test('keeps external import specifiers while permitting local aliases', () => {
  rejects('import { x as e } from "a";', 'import { y as e } from "a";', /interface/i);
  passes('import { e } from "a"; e();', 'import { e as execute } from "a"; execute();');
  passes('import e from "a"; e();', 'import execute from "a"; execute();');
});

test('keeps module loading order, including reexports', () => {
  rejects('import "a"; import "b";', 'import "b"; import "a";', /load.order/i);
  rejects('export * from "a"; export * from "b";', 'export * from "b"; export * from "a";', /load.order/i);
  passes('import "a"; import { x as e } from "b"; e();', 'import "a"; import { x as execute } from "b"; execute();');
});

test('rejects new globals and tolerates existing globals and standard constants', () => {
  rejects('let e = 1; use(e);', 'let count = 1; use(e);', /new.global/i);
  passes('let e = undefined; use(e, Infinity, NaN);', 'let count = undefined; use(count, Infinity, NaN);');
  passes('let e; missing = e;', 'let count; missing = count;');
});

test('rejects additional const assignments', () => {
  rejects('const e = 1; let t = 2; t++;', 'const value = 1; let t = 2; value++;', /const.reassigned/i);
  passes('const e = 1; let t = 2; t++; use(e);', 'const value = 1; let count = 2; count++; use(value);');
});

test('rejects early lexical reads but excludes export declarations and closures', () => {
  rejects('let e = 1; { use(e); let t = 2; }', 'let value = 1; { use(local); let local = 2; }', /early.use/i);
  passes('export const X = 1;');
  passes('const f = () => e; let e = 1;', 'const getValue = () => value; let value = 1;');
  passes('function f() { let e = 1; return e; }', 'function read() { let value = 1; return value; }');
});

test('preserves exact leading comments, including block comments', () => {
  rejects('// copyright\nlet e = 1;', '// changed\nlet count = 1;', /header/i);
  rejects('/* license */\n\nlet e = 1;', '/* license */\nlet count = 1;', /header/i);
  passes('/* license */\n// source\nlet e = 1;', '/* license */\n// source\nlet count = 1;');
});

test('rejects a renamed binding capturing references to an outer binding', () => {
  rejects('let e = 1; function f(t) { return e + t; }', 'let e = 1; function f(e) { return e + e; }', /binding/i);
  passes('let e = 1; function f(t) { return e + t; }', 'let value = 1; function sum(addend) { return value + addend; }');
});

test('rejects a renamed binding capturing a global reference', () => {
  rejects('function f(e) { return Math.max(e, 1); }', 'function f(Math) { return Math.max(Math, 1); }', /binding/i);
  passes('function f(e) { return Math.max(e, 1); }', 'function clamp(value) { return Math.max(value, 1); }');
});

test('tracks writes as well as reads and permits independent bindings to share a name', () => {
  rejects('let e; function f(t) { e = t; }', 'let e; function f(e) { e = e; }', /binding/i);
  passes('function f(e) { return e; } function g(t) { return t; }', 'function first(value) { return value; } function second(value) { return value; }');
});

test('normalizes object and pattern shorthand, defaults, imports and exports together', () => {
  passes('import { e } from "a"; const { t = e } = source; use({ e, t }); export { t };', 'import { e as initial } from "a"; const { t: value = initial } = source; use({ e: initial, t: value }); export { value as t };');
  passes('const f = ({ e }) => ({ e });', 'const make = ({ e: value }) => ({ e: value });');
  rejects('let e = 1; use({ e });', 'let value = 1; use({ value });', /structure|text/i);
});

test('rejects all other structure and text edits', () => {
  rejects('let e = 1;', 'let value = 2;', /structure|text/i);
  rejects('let e = 1;', 'let value=1;', /structure|text/i);
  rejects('let e = 1; // keep\nuse(e);', 'let value = 1; // changed\nuse(value);', /structure|text/i);
  passes('let e = 1; // keep\nuse(e);', 'let value = 1; // keep\nuse(value);');
});

test('preserves whitespace and comments inside explicit properties and aliases', () => {
  rejects('let e; use({ key: e });', 'let value; use({ key:value });', /structure|text/i);
  rejects('import { x as e } from "a";', 'import { x  as value } from "a";', /structure|text/i);
  rejects('let e; export { e as x };', 'let value; export { value  as x };', /structure|text/i);
  passes('let e; use({ key:/* keep */e });', 'let value; use({ key:/* keep */value });');
  passes('import { x /* keep */ as e } from "a";', 'import { x /* keep */ as value } from "a";');
});

test('matches shorthand references to declarations even when a name captures an outer value', () => {
  rejects('let e; function f(t) { return { e, t }; }', 'let e; function f(e) { return { e, t: e }; }', /binding/i);
  passes('let e; function f(t) { return { e, t }; }', 'let value; function make(item) { return { e: value, t: item }; }');
});

test('accounts for redeclarations, assignment patterns, catch scopes and loop bindings', () => {
  passes('var e; var e; ({ e } = source);', 'var value; var value; ({ e: value } = source);');
  passes('try {} catch (e) { use(e); } for (let t of items) use(t);', 'try {} catch (error) { use(error); } for (let item of items) use(item);');
  passes('function f({ e = 1, ...t }) { return [e, t]; }', 'function read({ e: value = 1, ...rest }) { return [value, rest]; }');
});
