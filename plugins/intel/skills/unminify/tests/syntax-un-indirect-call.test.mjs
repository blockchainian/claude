// Ported from Wakaru v1.14.0 (Apache-2.0); independently authored examples.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { indexFragmentContext } from '../scripts/fragments.mjs';
import rule from '../scripts/syntax/rules/un-indirect-call.mjs';

const run = (source, options = {}) => transformSyntax(source, { rules: [rule], ...options }).source;

test('UnIndirectCall removes zero sequences and global Object wrappers', () => {
  for (const [source, expected] of [
    ['(0, dispatch)(payload);', 'dispatch(payload);'],
    ['((0, (0, dispatch)))(payload);', '((0, (0, dispatch)))(payload);'],
    ['(0x0, ((service.send)))(...items);', 'service.send(...items);'],
    ['(0.0, service[slot])(payload);', 'service[slot](payload);'],
    ['Object(dispatch)(payload);', 'dispatch(payload);'],
    ['((Object))((service.send))(payload);', 'service.send(payload);'],
    ['Object(service[slot])(...items);', 'service[slot](...items);'],
    ['(0, service.send)((0, dispatch)(payload));', 'service.send(dispatch(payload));'],
    ['(0, (Object(service).send))(payload);', 'Object(service).send(payload);'],
  ]) assert.equal(run(source), expected, source);
});

test('UnIndirectCall retains nonmatching calls byte for byte', () => {
  const source = '// keep\r\nconsume((1, dispatch)(), (-0, dispatch)(), ((0), dispatch)(), (0, 1, dispatch)(), (0, 0)(), (0, dispatch())(), Object()(), Object(dispatch, extra)(), Object(...items)(), Object(42)(), new Object(dispatch)(), holder.Object(dispatch)(), (0, eval)(code), Object(eval)(code), (0, service.send)?.());\r\n';
  assert.equal(run(source), source);
});

test('UnIndirectCall respects lexical Object bindings and leaves unrelated scopes alone', () => {
  for (const declaration of ['function local(Object)', 'function local() { let Object; return function inner()']) {
    const source = declaration === 'function local(Object)'
      ? `${declaration} { return Object(dispatch)(payload); }`
      : `${declaration} { return Object(dispatch)(payload); }; }`;
    assert.equal(run(source), source);
  }
  assert.equal(run('function local(Object) {} Object(dispatch)(payload);'), 'function local(Object) {} dispatch(payload);');
  assert.equal(run('import Object from "box"; Object(dispatch)(payload);'), 'import Object from "box"; Object(dispatch)(payload);');
});

test('UnIndirectCall with scopes retain identifiers but allow member calls as Wakaru does', () => {
  const source = 'with (scope) { (0, dispatch)(payload); Object(dispatch)(payload); (0, service.send)(payload); Object(service.send)(payload); }';
  assert.equal(run(source), 'with (scope) { (0, dispatch)(payload); Object(dispatch)(payload); service.send(payload); service.send(payload); }');
});

test('UnIndirectCall indexes receiver-sensitive object definitions and assignments', () => {
  for (const definition of [
    'let service = { send() { return this.value; } };',
    'let service = { "send": function () { return this.value; } };',
    'let service; service = ({ send() { return this.value; } });',
    'let service = {}; service.send = (function () { return this.value; });',
    'let service = {}; (service)[("send")] = function () { return this.value; };',
    'let service = { ...source, send() { return this.value; } };',
  ]) {
    const source = `${definition} (0, service.send)(payload); Object(service["send"])(payload);`;
    assert.equal(run(source), source, definition);
  }
});

test('UnIndirectCall indexes static class methods in declarations, values and assignments', () => {
  for (const definition of [
    'class Sender { static send() { return this.value; } }',
    'let Sender = class { static "send"() { return super.value; } };',
    'let Sender; Sender = class { static send() { return this.value; } };',
    'class Sender { static get send() { return this.value; } }',
    'class Sender { static set send(value) { this.value = value; } }',
  ]) {
    const source = `${definition} (0, Sender.send)(payload); Object(Sender.send)(payload);`;
    assert.equal(run(source), source, definition);
  }
});

test('UnIndirectCall receiver observation includes parameters, arrows and direct eval', () => {
  for (const method of [
    'send(value = this.value) {}',
    'send() { return () => this.value; }',
    'send() { return eval(code); }',
    'send() { return eval("this.value"); }',
    'send() { return eval(...code); }',
    'send() { return eval(("safe")); }',
    'send() { return eval("thisValue", this.value); }',
    'send() { return { [this.value]() {} }; }',
    'send() { return class { [this.value]() {} }; }',
    'send() { return { [eval(code)]() {} }; }',
    'send() { return { [() => this.value]() {} }; }',
  ]) {
    const source = `const service = { ${method} }; (0, service.send)(payload);`;
    assert.equal(run(source), source, method);
  }
});

test('UnIndirectCall skips nested ordinary receivers and receiver-independent eval', () => {
  for (const method of [
    'send() { return function () { return this.value; }; }',
    'send() { return { nested() { return this.value; } }; }',
    'send() { return { [function() { return this.value; }]() {} }; }',
    'send() { return eval(); }',
    'send() { return eval("thisValue"); }',
    'send() { return eval("quire".replace(/^/, "re")); }',
    'send() { return eval("quire".replace(.../^/, ..."re")); }',
    'send: () => this.value',
    '["send"]() { return this.value; }',
    'get send() { return this.value; }',
  ]) {
    const prefix = `const service = { ${method} }; `;
    assert.equal(run(`${prefix}(0, service.send)(payload);`), `${prefix}service.send(payload);`, method);
  }
  assert.equal(run('class Sender { send() { return this; } } (0, Sender.send)();'), 'class Sender { send() { return this; } } Sender.send();');
  assert.equal(run('class Sender { static send = function () { return this; }; } (0, Sender.send)();'), 'class Sender { static send = function () { return this; }; } Sender.send();');
});

test('UnIndirectCall receiver index uses binding identity and static member names', () => {
  const prefix = 'const service = { send() { return this; } }; function local(service) { ';
  assert.equal(run(`${prefix}(0, service.send)(); } (0, service.send)();`), `${prefix}service.send(); } (0, service.send)();`);
  assert.equal(run('const service = { send() { return this; } }; (0, service[key])();'), 'const service = { send() { return this; } }; service[key]();');
  assert.equal(run('let service = {}; service.send ||= function () { return this; }; (0, service.send)();'), 'let service = {}; service.send ||= function () { return this; }; service.send();');
  const source = 'class Sender { static send() { return this; } static forward() { return (0, Sender.send)(); } }';
  assert.equal(run(source), source);
});

test('UnIndirectCall fragments retain unknown receivers and Object proof while preserving declarations', () => {
  const source = 'var service = source, result = (0, dispatch)(payload); const stable = 1; function local() { let value = (0, dispatch)(); return (0, remote.send)(value); } Object(dispatch)(result); (0, service.send)(result);';
  const expected = source.replace('(0, dispatch)(payload)', 'dispatch(payload)').replace('(0, dispatch)()', 'dispatch()');
  const fragmentContext = indexFragmentContext(new Map([['sibling.js', 'var Object; remote.send = function () { return this; };']]));
  assert.equal(run(source, { fragment: true, fragmentContext }), expected);
  assert.equal(run('export { shared }; (0, remote.send)();'), 'export { shared }; (0, remote.send)();');
});

test('UnIndirectCall fragments allow complete local function receivers', () => {
  const source = 'function local() { const service = { send() { return 3; } }; return (0, service.send)(); }';
  assert.equal(run(source, { fragment: true }), source.replace('(0, service.send)', 'service.send'));
});

test('UnIndirectCall fragments retain local aliases and parameters with unknown method definitions', () => {
  for (const source of [
    'function local(service) { return (0, service.send)(); }',
    'function local() { const service = remote; return (0, service.send)(); }',
    'function local() { const service = { send: remote }; return (0, service.send)(); }',
    'function local() { const service = { ...remote }; return (0, service.send)(); }',
  ]) assert.equal(run(source, { fragment: true }), source);
});

test('UnIndirectCall preserves bytes outside edited callee ranges and handles nesting', () => {
  assert.equal(run('// retain\r\nconst  value = (0, dispatch)( /* arg */ payload );\r\n'), '// retain\r\nconst  value = dispatch( /* arg */ payload );\r\n');
  assert.equal(run('Object((0, maker)().send)(payload);'), 'maker().send(payload);');
  assert.equal(run('(0, Object(dispatch))(payload);'), '(0, Object(dispatch))(payload);');
});

test('UnIndirectCall keeps leading object, function and class members valid as statements', () => {
  for (const value of ['{ send: dispatch }.send', 'function(){}.call', 'async/**/function(){}.call', 'class { static send() {} }.send', 'let[key]', 'let/* key */[key]']) {
    assert.equal(run(`(0, ${value})();`), `(${value})();`, value);
    assert.equal(run(`Object(${value})();`), `(${value})();`, value);
    assert.equal(run(`consume((0, ${value})());`), `consume(${value}());`, value);
  }
});
