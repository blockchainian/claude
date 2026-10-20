// Ported from wakaru v1.14.0 (Apache-2.0); these test inputs are independently authored.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/bracket-notation.mjs';

const run = (source, fragment = false) => transformSyntax(source, { rules: [rule], fragment }).source;

test('UnBracketNotation restores member IdentifierNames, including keywords and Unicode', () => {
  assert.equal(run('read(box["return"], box["await"], box["$field"], box["_field"], box["温度"], box["a\\u200Cb"], box["\\u0078"]);'),
    'read(box.return, box.await, box.$field, box._field, box.温度, box.a\u200Cb, box.x);');
});

test('UnBracketNotation visits optional, nested, super, delete and assignment members', () => {
  const source = 'box["ready"]["state"] = box?.["next"]["value"]; delete box["stale"]; class Child extends Base { read(){ return super["read"]?.["name"]; } }';
  assert.equal(run(source), 'box.ready.state = box?.next.value; delete box.stale; class Child extends Base { read(){ return super["read"]?.name; } }');
});

test('UnBracketNotation retains super property expressions handled separately by Wakaru', () => {
  const source = 'class Child extends Parent { read(){ return [super["status"], super["7"], super["bad-key"]]; } }';
  assert.equal(run(source), source);
});

test('UnBracketNotation keeps numeric objects syntactically valid', () => {
  assert.equal(run('read(7["toString"](), 7.25["toString"](), 0xF["toString"](), 7n["toString"](), (7)["toString"]());'),
    'read(7 .toString(), 7.25.toString(), 0xF.toString(), 7n.toString(), (7).toString());');
});

test('UnBracketNotation separates dotted members from decimal integers with digit separators', () => {
  assert.equal(run('1_024["toString"]();'), '1_024 .toString();');
});

test('UnBracketNotation restores normalized decimal member keys at both boundaries', () => {
  for (const value of ['0', '39', '8.125', '0.000001', '999999999999999900000', '1000000000000000100']) {
    assert.equal(run(`box["${value}"]; box?.["${value}"];`), `box[${value}]; box?.[${value}];`);
  }
});

test('UnBracketNotation rejects noncanonical decimal keys and invalid IdentifierNames', () => {
  for (const value of ['', '01', '4.0', '.4', '4.', '-0', '-4', '+4', '4e2', '0xff', ' 4', '4 ', '0.0000001', '1000000000000000000000', '9007199254740993', '1.2.3', '2cats', 'dash-key', 'a b', '\\uD800']) {
    const source = `box["${value}"]; ({"${value}": 4, ["${value}"]: 5});`;
    assert.equal(run(source), source, value);
  }
});

test('UnBracketNotation converts object data keys, methods, accessors and numeric names', () => {
  const source = 'const table = { "await": 2, ["status"]: 3, "7.25": 4, ["8"]: 5, "read"(){}, ["write"](){}, get ["value"](){return 1}, set "value"(v){} };';
  assert.equal(run(source), 'const table = { await: 2, status: 3, 7.25: 4, 8: 5, read(){}, write(){}, get value(){return 1}, set value(v){} };');
});

test('UnBracketNotation converts class fields, methods and accessors', () => {
  const source = 'class Store { ["record"] = 3; static "size" = 4; ["9"](){}; async ["read"](){}; *"walk"(){}; get ["value"](){return this["record"]} set ["value"](v){this["record"]=v} }';
  assert.equal(run(source), 'class Store { record = 3; static size = 4; 9(){}; async read(){}; *walk(){}; get value(){return this.record} set value(v){this.record=v} }');
});

test('UnBracketNotation protects computed special keys in every property-name position', () => {
  const source = 'const table = { ["__proto__"]: 3, ["constructor"](){}, get ["prototype"](){return 2} }; class Store { ["__proto__"] = 4; ["constructor"](){}; static ["prototype"](){} } const {["__proto__"]: saved} = table;';
  assert.equal(run(source), source);
});

test('UnBracketNotation converts noncomputed special names and special member names', () => {
  assert.equal(run('const table = {"__proto__": null, "constructor": 3, "prototype": 4}; read(table["__proto__"], table["constructor"], table["prototype"]);'),
    'const table = {__proto__: null, constructor: 3, prototype: 4}; read(table.__proto__, table.constructor, table.prototype);');
});

test('UnBracketNotation preserves dynamic and parenthesized key expressions byte for byte', () => {
  const source = 'read(box[key], box[7], box[`name`], box[("name")], box["a" + "b"], box?.[("ready")]); const table = {[("name")]: 7, [key]: 8, 12: 9};';
  assert.equal(run(source), source);
});

test('UnBracketNotation preserves comments, whitespace and untouched text', () => {
  const source = '// keep\r\nconst  item = box /* before */ [ /* [ */ "ready" /* ] */ ];\r\nconst table = { [ /* [ */ "status" /* ] */ ] : 5 };\r\n';
  assert.equal(run(source), '// keep\r\nconst  item = box /* before */ . /* [ */ ready /* ] */ ;\r\nconst table = {  /* [ */ status /* ] */  : 5 };\r\n');
});

test('UnBracketNotation preserves line comments around key delimiters', () => {
  assert.equal(run('box[// [\n"ready"// ]\n]; ({[// [\n"status"// ]\n]: 2});'),
    'box.// [\nready// ]\n; ({// [\nstatus// ]\n: 2});');
});

test('UnBracketNotation recognizes Unicode line terminators inside comments', () => {
  const source = '({["name"// ]\u2028]: box["value"]});';
  assert.equal(run(source), '({name// ]\u2028: box.value});');
});

test('UnBracketNotation separates compact modifiers from restored property names', () => {
  assert.equal(run('({get["size"](){return 4},set["size"](v){},async["read"](){}}); class Store {static["size"]=4;get["count"](){return 3}}'),
    '({get size(){return 4},set size(v){},async read(){}}); class Store {static size=4;get count(){return 3}}');
});

test('UnBracketNotation handles nested property values without overlapping edits', () => {
  assert.equal(run('({["outer"]: {["inner"]: box["left"]["right"]}})["outer"];'),
    '({outer: {inner: box.left.right}}).outer;');
});

test('UnBracketNotation preserves fragment binding names, kinds and declaration layout', () => {
  const source = 'var {"status": state, ["7"]: count = box["fallback"]} = input, other = 1; let {"next": next} = input; const {"done": done} = input; export { missing };';
  assert.equal(run(source, true), 'var {status: state, 7: count = box.fallback} = input, other = 1; let {next: next} = input; const {done: done} = input; export { missing };');
});

test('UnBracketNotation adds no globals even in fragments with shadowed intrinsics', () => {
  assert.equal(run('const Infinity = 1, undefined = 2; box["Infinity"]; box["undefined"]; box["NaN"];', true),
    'const Infinity = 1, undefined = 2; box.Infinity; box.undefined; box.NaN;');
});

test('UnBracketNotation leaves import/export names and unrelated strings unchanged', () => {
  const source = 'import { "return" as value } from "pkg"; export { value as "return" }; const text = "status"; call("ready");';
  assert.equal(run(source), source);
});
