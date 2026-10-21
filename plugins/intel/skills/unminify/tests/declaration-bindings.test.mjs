import test from 'node:test';
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
import { resolveNames, applyRenames } from '../scripts/rename.mjs';
import { checkRewrite } from '../scripts/checks.mjs';
import { checkChunkRewrite } from '../scripts/chunks.mjs';

const guards = {
  file: (before, after) => checkRewrite(before, after),
  chunk: (before, after) => checkChunkRewrite(new Map([['sample.js', before]]), new Map([['sample.js', after]])),
};
const fixtures = [
  { name: 'parameter', source: 'function room(x) { return x(); }\nroom(() => 7);', key: 14, target: 'room' },
  { name: 'body-local', source: 'function enabled() { let x = () => 7; return x(); }\nenabled();', key: 25, target: 'enabled' },
];
for (const fixture of fixtures) for (const [name, guard] of Object.entries(guards)) {
  test(`${name} guard accepts ${fixture.name} shadowing without a suffix`, () => {
    assert.equal(fixture.source.slice(fixture.key, fixture.key + 1), 'x');
    const resolved = resolveNames(fixture.source, new Map([[fixture.key, fixture.target]]));
    assert.equal(resolved.get(fixture.key), fixture.target);
    const after = applyRenames(fixture.source, resolved);
    assert.equal(runInNewContext(fixture.source), 7);
    assert.equal(runInNewContext(after), 7);
    assert.equal(guard(fixture.source, after), null);
  });
}
for (const [name, guard] of Object.entries(guards)) {
  test(`${name} guard keeps real recursive references protected`, () => {
    const source = 'function room(x) { return room(x - 1); }\n';
    const resolved = resolveNames(source, new Map([[14, 'room']]));
    assert.equal(resolved.get(14), 'room2');
    assert.equal(guard(source, applyRenames(source, resolved)), null);
    assert.ok(guard(source, applyRenames(source, new Map([[14, 'room']]))));
  });
  test(`${name} guard still rejects captured writes`, () => {
    const source = 'let outer = 0; function room(x) { outer = x; return outer; }';
    const after = source.replace('room(x)', 'room(outer)').replace('outer = x;', 'outer = outer;');
    assert.ok(guard(source, after));
  });
}

for (const fixture of fixtures) {
  test(`inverse ${fixture.name} rename preserves the outer function declaration`, () => {
    const after = applyRenames(fixture.source, new Map([[fixture.key, fixture.target]]));
    assert.equal(applyRenames(after, new Map([[fixture.key, 'x']])), fixture.source);
  });
}
