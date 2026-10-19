import assert from 'node:assert/strict';
import test from 'node:test';
import { parse } from '../scripts/node_modules/@babel/parser/lib/index.js';
import traverseModule from '../scripts/node_modules/@babel/traverse/lib/index.js';
import { toIdentifier, resolveNames, applyRenames, followImports } from '../scripts/rename.mjs';
const traverse = traverseModule.default ?? traverseModule;
const ast = source => parse(source, { sourceType: 'unambiguous', plugins: ['jsx'] });
function bindings(source) {
  const found = new Map();
  traverse(ast(source), { Scopable(path) {
    for (const binding of Object.values(path.scope.bindings)) found.set(binding.identifier.start, binding);
  } });
  return [...found.values()].sort((a, b) => a.identifier.start - b.identifier.start);
}
function key(source, name, occurrence = 0) {
  const binding = bindings(source).filter(b => b.identifier.name === name)[occurrence];
  assert.ok(binding, `missing ${name}`);
  return binding.identifier.start;
}
function names(source, entries) {
  return new Map(entries.map(([name, value, occurrence]) => [key(source, name, occurrence), value]));
}
// Compare binding identities by declaration order and references by AST traversal order.
// Shorthand expansion adds property keys, so only actual references enter this list.
function referenceTargets(source) {
  const tree = ast(source);
  const declarations = bindings(source);
  const order = new Map(declarations.map((b, i) => [b.identifier.start, i]));
  const targets = [];
  traverse(tree, { 'Identifier|JSXIdentifier'(path) {
    if (!path.isReferencedIdentifier() && !path.isBindingIdentifier()) return;
    if (path.parentPath.isImportSpecifier() && path.key === 'imported') return;
    if (path.parentPath.isObjectProperty() && path.key === 'key' && !path.parent.computed) return;
    // JSX member properties and intrinsic element names are not lexical references.
    if (path.isJSXIdentifier() && (path.parentPath.isJSXMemberExpression() ? path.key === 'property' : /^[a-z]/.test(path.node.name))) return;
    if (path.parentPath.isExportSpecifier() || path.parentPath.isExportNamedDeclaration()) return;
    const binding = path.scope.getBinding(path.node.name);
    targets.push(binding ? order.get(binding.identifier.start) : `global:${path.node.name}`);
  } });
  return targets;
}
function equivalent(before, after) {
  assert.doesNotThrow(() => ast(after));
  assert.deepEqual(referenceTargets(after), referenceTargets(before));
}

test('normalizes separators, punctuation, Unicode, reserved words and invalid starts', () => {
  for (const [input, expected] of [
    ['foo.bar-- baz', 'fooBarBaz'], [' _foo!?bar ', '_foobar'], ['$value', '$value'],
    ['café-值', 'café值'], ['', '_'], ['💥!?', '_'], ['12items', '_12items'],
    ['\u0301value', '_\u0301value'], ['class', '_class'], ['await', '_await'],
    ['let', '_let'], ['eval', '_eval'], ['arguments', '_arguments'], ['async', 'async'],
  ]) {
    assert.equal(toIdentifier(input), expected);
    assert.doesNotThrow(() => ast(`const ${toIdentifier(input)} = 1; export {};`));
  }
});

test('checks same, ancestor, descendant bindings and global captures, but permits siblings', () => {
  const source = 'let value=0; function f(a){ function g(){let taken=1; return a+taken+globalValue} return a } function h(b){return b}';
  for (const [proposal, expected] of [['value', 'value2'], ['taken', 'taken2'], ['globalValue', 'globalValue2']]) {
    assert.equal(resolveNames(source, names(source, [['a', proposal]])).get(key(source, 'a')), expected);
  }
  const result = resolveNames(source, names(source, [['a', 'item'], ['b', 'item']]));
  assert.equal(result.get(key(source, 'a')), 'item');
  assert.equal(result.get(key(source, 'b')), 'item');
  equivalent(source, applyRenames(source, result));
});

test('reserves names within a batch, increments suffixes and omits unchanged proposals', () => {
  const source = 'let a=1,b=2,item2=3,item3=4; function f(c){return a+b+c}';
  const result = resolveNames(source, names(source, [['a', 'item2'], ['b', 'item2'], ['c', 'item4']]));
  assert.equal(result.get(key(source, 'a')), 'item4');
  assert.equal(result.get(key(source, 'b')), 'item5');
  assert.equal(result.get(key(source, 'c')), 'item6');
  assert.equal(resolveNames(source, names(source, [['a', 'a']])).size, 0);
  equivalent(source, applyRenames(source, result));
});

test('unrelated global references do not block a sibling rename', () => {
  const source = 'function f(a){return a} function g(){return external}';
  assert.equal(resolveNames(source, names(source, [['a', 'external']])).get(key(source, 'a')), 'external');
});

test('rewrites reads, assignments, updates and repeated var declarations without reprinting', () => {
  const source = '/* 😀 */ function f(e) { var a=e; var a; a+=e; a++; ({x:a}={x:2}); return { a, fixed: a, [a]: e }; }';
  const after = applyRenames(source, names(source, [['e', 'input'], ['a', 'count']]));
  assert.equal(after, '/* 😀 */ function f(input) { var count=input; var count; count+=input; count++; ({x:count}={x:2}); return { a: count, fixed: count, [count]: input }; }');
  equivalent(source, after);
});

test('expands destructuring shorthand including defaults and assignment patterns', () => {
  const source = 'function f({e=1, x:{a}, ...r}) { ({e}=r); return { e /*keep*/, a, r }; }';
  const after = applyRenames(source, names(source, [['e', 'entry'], ['a', 'amount'], ['r', 'rest']]));
  assert.equal(after, 'function f({e: entry=1, x:{a: amount}, ...rest}) { ({e: entry}=rest); return { e: entry /*keep*/, a: amount, r: rest }; }');
  equivalent(source, after);
});

test('preserves external import/export names and comments', () => {
  const source = 'import d, {e, f as g} from "./a.js"; import * as ns from "./b.js"; export { e, g as publicName }; console.log(d,e,g,ns);';
  const after = applyRenames(source, names(source, [['d', 'defaultValue'], ['e', 'entry'], ['g', 'other'], ['ns', 'namespace']]));
  assert.equal(after, 'import defaultValue, {e as entry, f as other} from "./a.js"; import * as namespace from "./b.js"; export { entry as e, other as publicName }; console.log(defaultValue,entry,other,namespace);');
  equivalent(source, after);
});

test('preserves direct declaration export names', () => {
  const source = 'export const e=1, {a}=obj; export function f(){return e+a} export class C {}';
  const after = applyRenames(source, names(source, [['e', 'entry'], ['a', 'amount'], ['f', 'read'], ['C', 'Component']]));
  const exported = [];
  traverse(ast(after), { ExportSpecifier(path) { exported.push(path.node.exported.name); } });
  assert.deepEqual(exported.sort(), ['C', 'a', 'e', 'f']);
  equivalent(source, after);
});

test('renames lexical JSX tags and roots, leaves properties and intrinsic tags alone', () => {
  const source = 'const C=load(), ns=load(); const e=<C prop={C}><ns.View/><div/></C>;';
  const after = applyRenames(source, names(source, [['C', 'Component'], ['ns', 'widgets'], ['e', 'element']]));
  assert.equal(after, 'const Component=load(), widgets=load(); const element=<Component prop={Component}><widgets.View/><div/></Component>;');
  equivalent(source, after);
});

test('unknown binding keys fail loudly', () => {
  assert.throws(() => resolveNames('let a=1;', new Map([[999, 'item']])));
  assert.throws(() => applyRenames('let a=1;', new Map([[999, 'item']])));
});

test('follows named and default exports, resolving collisions and relative paths', () => {
  const files = new Map([
    ['/project/lib/source.js', 'const entry=1; export {entry as e}; export default function readValue(){return entry}'],
    ['/project/main.js', 'import r,{e as x} from "./lib/source.js"; const entry=0; console.log(r,x,entry);'],
    ['/project/other.js', 'import {e as x} from "package"; console.log(x);'],
  ]);
  const result = followImports(files);
  assert.equal(result.get('/project/main.js'), 'import readValue,{e as entry2} from "./lib/source.js"; const entry=0; console.log(readValue,entry2,entry);');
  assert.equal(result.has('/project/lib/source.js'), false);
  assert.equal(result.has('/project/other.js'), false);
  equivalent(files.get('/project/main.js'), result.get('/project/main.js'));
});

test('follows re-exports, star exports, extensionless imports and cycles safely', () => {
  const files = new Map([
    ['src/a.js', 'const value=1; export {value as e}; export * from "./b.js";'],
    ['src/b.js', 'export {e as alias} from "./a.js"; export * from "./a.js";'],
    ['src/main.js', 'import {alias as a,e as b} from "./b"; console.log(a,b);'],
  ]);
  const result = followImports(files);
  assert.equal(result.get('src/main.js'), 'import {alias as value,e as value2} from "./b"; console.log(value,value2);');
  const again = new Map(files);
  for (const [file, source] of result) again.set(file, source);
  assert.equal(followImports(again).size, 0);
});

test('follows through imported exports with final local names', () => {
  const files = new Map([
    ['src/a.js', 'export const descriptive=1;'],
    ['src/b.js', 'import {descriptive as b} from "./a.js"; export {b as alias};'],
    ['src/c.js', 'import {alias as c} from "./b.js"; console.log(c);'],
  ]);
  const result = followImports(files);
  assert.match(result.get('src/b.js'), /as descriptive/);
  assert.match(result.get('src/c.js'), /as descriptive/);
});

test('does not capture global writes or destructuring assignment targets', () => {
  for (const source of [
    'let a=1; function f(){external=2; return a}',
    'let a=1; function f(){({external}=obj); return a}',
    'let a=1; function f(){for(external of items) use(a)}',
  ]) assert.equal(resolveNames(source, names(source, [['a', 'external']])).get(key(source, 'a')), 'external2');
});

test('keeps escaped shorthand public keys and Unicode offsets intact', () => {
  const source = '/* 😀 */ const \\u0065=1; export {\\u0065}; const object={\\u0065};';
  const after = applyRenames(source, names(source, [['e', 'entry']]));
  assert.equal(after, '/* 😀 */ const entry=1; export {entry as \\u0065}; const object={\\u0065: entry};');
  equivalent(source, after);
});

test('renames catch, loop, recursive class and nested shadowed bindings independently', () => {
  const source = 'const a=1; function f(a){try{throw a}catch(e){for(let i of [e]) use(i,a)}} class C {m(){return C}} use(a,C);';
  const proposals = names(source, [['a', 'outer', 0], ['a', 'inner', 1], ['e', 'error'], ['i', 'item'], ['C', 'Component']]);
  const after = applyRenames(source, resolveNames(source, proposals));
  equivalent(source, after);
  assert.match(after, /return Component/);
});

test('real open source fixture retains binding identities after batch renames', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = await readFile(new URL('./fixtures/preact-hooks.mjs', import.meta.url), 'utf8');
  const proposals = new Map(bindings(source).filter(b => !b.kind.startsWith('module')).map((b, i) => [b.identifier.start, `value${i}`]));
  const after = applyRenames(source, resolveNames(source, proposals));
  equivalent(source, after);
});

test('keeps JSX tag names lexical when a model proposes a lowercase name', () => {
  const source = 'const C=load(); const e=<C/>;';
  const resolved = resolveNames(source, names(source, [['C', 'component']]));
  assert.equal(resolved.get(key(source, 'C')), 'Component');
  equivalent(source, applyRenames(source, resolved));
  assert.throws(() => applyRenames(source, names(source, [['C', 'component']])), /JSX/);
});

test('splits direct exports without extra spaces or semicolons and keeps comments', () => {
  for (const [source, expected] of [
    ['export const e=1;', 'const entry=1;\nexport { entry as e };'],
    ['export function e(){}', 'function entry(){}\nexport { entry as e };'],
    ['export class e {}', 'class Entry {}\nexport { Entry as e };'],
    ['export /*keep*/ const e=1', '/*keep*/ const entry=1\nexport { entry as e };'],
  ]) {
    const proposed = source.includes('class') ? 'Entry' : 'entry';
    const after = applyRenames(source, names(source, [['e', proposed]]));
    assert.equal(after, expected);
    equivalent(source, after);
  }
});

test('public export and re-export names are not unresolved global references', () => {
  const source = 'const a=1,b=2; export {a as publicName}; export {remoteName as other} from "./other.js";';
  for (const proposed of ['publicName', 'remoteName', 'other']) {
    assert.equal(resolveNames(source, names(source, [['b', proposed]])).get(key(source, 'b')), proposed);
  }
});
