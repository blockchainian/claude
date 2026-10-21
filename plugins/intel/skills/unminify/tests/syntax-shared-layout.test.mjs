import test from 'node:test';
import assert from 'node:assert/strict';
import { checkSyntaxRewrite } from '../scripts/syntax/checks.mjs';

test('fragments allow splitting and merging declarations without changing evaluation order', () => {
  for (const kind of ['var', 'let', 'const']) {
    const grouped = `before(); ${kind} first=readFirst(), second=readSecond(); after();`;
    const split = `before(); ${kind} first=readFirst(); ${kind} second=readSecond(); after();`;
    assert.equal(checkSyntaxRewrite(grouped, split, { fragment: true }), null);
    assert.equal(checkSyntaxRewrite(split, grouped, { fragment: true }), null);
  }
});

test('fragment initializer prefixes may become statements in the same evaluation position', () => {
  for (const [before, after] of [
    ['var first=(prepare(), read()), second=finish();', 'prepare(); var first=read(); var second=finish();'],
    ['if (ready) { var shared=(prepare(), read()); }', 'if (ready) { prepare(); var shared=read(); }'],
    ['for (var shared=(prepare(), read()); ready; step()) {}', 'prepare(); for (var shared=read(); ready; step()) {}'],
    ['if (prepare(), ready) { var shared=read(); }', 'prepare(); if (ready) { var shared=read(); }'],
    ['function run(){ let local=(prepare(), read()); return local; } var shared=run();',
      'function run(){ prepare(); let local=read(); return local; } var shared=run();'],
    ['var shared; before(), after();', 'var shared; before(); after();'],
  ]) assert.equal(checkSyntaxRewrite(before, after, { fragment: true }), null, before);
});

test('fragment checks reject binding, kind, initialization, statement and control-order changes', () => {
  for (const [before, after] of [
    ['var first=read(), second=read();', 'var second=read(); var first=read();'],
    ['before(); var shared=read(); after();', 'after(); var shared=read(); before();'],
    ['record("first"); var shared=read(); record("last");', 'record("last"); var shared=read(); record("first");'],
    ['var shared; shared=1; shared=2;', 'var shared; shared=2; shared=1;'],
    ['var shared=read(); after();', 'after(); var shared=read();'],
    ['var shared=(prepare(), read());', 'var shared=read(); prepare();'],
    ['if (ready) { var shared=read(); }', 'if (ready) {} var shared=read();'],
    ['var shared=read();', 'let shared=read();'],
    ['var shared=read();', 'var renamed=read();'],
    ['var first=read(), second=again();', 'var first=read(); again();'],
    ['var shared=read(), shared=again();', 'var shared=again();'],
    ['var first=read(), second=again();', 'var [first,second]=[read(),again()];'],
  ]) assert.match(checkSyntaxRewrite(before, after, { fragment: true }), /FRAGMENT/, before);
});

test('literal and member spelling rewrites retain fragment evaluation order', () => {
  assert.equal(checkSyntaxRewrite('var shared=!0; consume(record["name"]);',
    'var shared=true; consume(record.name);', { fragment: true }), null);
});

test('computed method keys retain their evaluation events without inspecting method bodies', () => {
  const source = 'var methods={ [name](){ return !0; }, [key()](){} };';
  assert.equal(checkSyntaxRewrite(source, source.replace('!0', 'true'), { fragment: true }), null);
  assert.match(checkSyntaxRewrite(source, 'var methods={ [key()](){}, [name](){ return !0; } };',
    { fragment: true }), /FRAGMENT/);
});
