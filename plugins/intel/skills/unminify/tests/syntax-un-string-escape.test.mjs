// Ported from wakaru v1.14.0 (Apache-2.0); examples are independently authored.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/un-string-escape.mjs';

const run = (source, options = {}) => transformSyntax(source, { ...options, rules: [rule] }).source;

test('UnStringEscape decodes fixed, braced, hex and surrogate pair escapes', () => {
  assert.equal(run(String.raw`consume('\u03bb', "\u{1F680}", '\xE9', '\uD83C\uDF0D');`),
    `consume('λ', "🚀", 'é', '🌍');`);
});

test('UnStringEscape accepts mixed hex case, leading zeroes and braced scalar boundaries', () => {
  assert.equal(run(String.raw`consume('\u00eD\u00ED\u{000003bb}\u{10000}\u{323AF}');`),
    `consume('ííλ${String.fromCodePoint(0x10000, 0x323af)}');`);
});

test('UnStringEscape preserves ASCII, escaped quotes and unrelated escapes', () => {
  const source = String.raw`consume("\u0062\x62\u{62}\"\'\\\n\r\t\b\f\v\0\u03bb");`;
  assert.equal(run(source), source.replace(String.raw`\u03bb`, 'λ'));
});

test('UnStringEscape respects backslash parity', () => {
  const source = String.raw`consume("\\u03bb", "\\xE9", "\\\u03bb", "\\\\u03bb");`;
  assert.equal(run(source), String.raw`consume("\\u03bb", "\\xE9", "\\λ", "\\\\u03bb");`);
});

test('UnStringEscape preserves lone surrogates and nonpair neighbours', () => {
  const source = String.raw`consume("\uD901!\uDC42\uD902\u03bb\u{D903}\u{DC43}\uD904\u0042\uD905\uD906");`;
  assert.equal(run(source), source.replace(String.raw`\u03bb`, 'λ'));
});

test('UnStringEscape keeps invisible supplementary surrogate pairs intact', () => {
  const source = String.raw`consume('\uDB80\uDC42\uD82F\uDCA0\uD834\uDD73');`;
  assert.equal(run(source), source);
});

test('UnStringEscape keeps surrogate pairs adjacent to range hyphens escaped', () => {
  const source = String.raw`consume("a-\uD83D\uDE80", "\uD83D\uDE80-z", "\uD83D\uDE80", "-\u{1F680}-");`;
  assert.equal(run(source), String.raw`consume("a-\uD83D\uDE80", "\uD83D\uDE80-z", "🚀", "-🚀-");`);
});

test('UnStringEscape applies Wakaru range protection to twelve-character braced escapes', () => {
  const source = String.raw`consume('-\u{0001F680}', '\u{0001F680}-', '\u{0001F680}');`;
  assert.equal(run(source), String.raw`consume('-\u{0001F680}', '\u{0001F680}-', '🚀');`);
});

const invisibleRanges = [
  [0x80, 0x9f], [0xa0, 0xa0], [0xad, 0xad], [0x34f, 0x34f], [0x61c, 0x61c],
  [0x115f, 0x1160], [0x17b4, 0x17b5], [0x180b, 0x180f], [0x2000, 0x200f],
  [0x2028, 0x202f], [0x205f, 0x206f], [0x3000, 0x3000], [0x3164, 0x3164],
  [0xe000, 0xf8ff], [0xfe00, 0xfe0f], [0xfeff, 0xfeff], [0xffa0, 0xffa0],
  [0xfff0, 0xffff], [0x1bca0, 0x1bca3], [0x1d173, 0x1d17a], [0x323b0, 0x10ffff],
];
for (const [start, end] of invisibleRanges) {
  test(`UnStringEscape retains invisible range ${start.toString(16)}..${end.toString(16)}`, () => {
    const escapes = [...new Set([start, end, Math.floor((start + end) / 2)])]
      .map(value => `\\u{${value.toString(16)}}`).join('');
    const source = `consume('${escapes}');`;
    assert.equal(run(source), source);
  });
}

test('UnStringEscape follows Rust Unicode whitespace rather than JS whitespace', () => {
  const source = String.raw`consume('\u0085\u1680\u2028\u2029\u{2007}\u{202F}\u180A\u1810');`;
  assert.equal(run(source), source.replace(String.raw`\u180A\u1810`, '\u180a\u1810'));
});

test('UnStringEscape retains the exact printable boundaries surrounding invisible ranges', () => {
  const points = [0xac, 0xae, 0x34e, 0x350, 0x61b, 0x61d, 0x115e, 0x1161,
    0x17b3, 0x17b6, 0x180a, 0x1810, 0x200a, 0x2010, 0x2027, 0x2030,
    0x205e, 0x2070, 0x3163, 0x3165, 0xd7ff, 0xf900, 0xfdff, 0xfe10,
    0xfefe, 0xff00, 0xff9f, 0xffa1, 0xffef, 0x10000, 0x1bc9f, 0x1bca4,
    0x1d172, 0x1d17b, 0x323af];
  const escaped = points.map(value => `\\u{${value.toString(16)}}`).join('');
  // U+200A is whitespace; all remaining neighbours are printable by this rule.
  const expected = points.map(value => value === 0x200a ? '\\u{200a}' : String.fromCodePoint(value)).join('');
  assert.equal(run(`consume('${escaped}');`), `consume('${expected}');`);
});

test('UnStringEscape preserves escaped line continuations', () => {
  const source = "consume('\\\r\n\\u03bb\\\n\\xE9');";
  assert.equal(run(source), "consume('\\\r\nλ\\\né');");
});

test('UnStringEscape retains printable original text and every outside byte', () => {
  const source = "// \\u03bb\r\nconst  text = 'café\\u03bb'; /* \\xE9 */\r\nconsume( text );\r\n";
  assert.equal(run(source), source.replace("café\\u03bb", 'caféλ'));
});

test('UnStringEscape leaves regex and template raw text while visiting substitutions', () => {
  const source = 'consume(/\\u03bb/, `\\u03bb`, tag`\\u03bb${"\\u03bb"}`);';
  assert.equal(run(source), 'consume(/\\u03bb/, `\\u03bb`, tag`\\u03bb${"λ"}`);');
});

test('UnStringEscape decodes keys, module specifiers and directive strings', () => {
  const source = String.raw`"\u03bb"; import value from './\u03bb.js'; const obj = {'\u03bb': '\xE9'}; export {value as '\u03bb'};`;
  assert.equal(run(source), `"λ"; import value from './λ.js'; const obj = {'λ': 'é'}; export {value as 'λ'};`);
});

test('UnStringEscape preserves fragment bindings, declaration kinds and grouping', () => {
  const source = String.raw`var shared = '\u03bb', other = '\xE9'; let next; const fixed = '\u{1F680}'; function read() { return '\u03bb'; } export {missing};`;
  assert.equal(run(source, { fragment: true }), `var shared = 'λ', other = 'é'; let next; const fixed = '🚀'; function read() { return 'λ'; } export {missing};`);
});

test('UnStringEscape needs no global names even with shadowing and dynamic scope', () => {
  const source = String.raw`var String, undefined, Infinity; with (record) { consume('\u03bb'); } eval(code);`;
  assert.equal(run(source, { fragment: true }), source.replace(String.raw`\u03bb`, 'λ'));
});

test('UnStringEscape returns no edits for unchanged text and is idempotent', () => {
  const source = String.raw`consume('plain', 'é', '\\u03bb', '\x3A', '\u0022', '\u005C');`;
  assert.equal(run(source), source);
  assert.deepEqual(transformSyntax(source, { rules: [rule] }).steps, [{ name: 'UnStringEscape', edits: [] }]);
  const changed = run(String.raw`consume('\u03bb');`);
  assert.equal(run(changed), changed);
});

test('UnStringEscape preserves JSX attribute string spelling through the transform API', () => {
  const source = String.raw`const badge = <Badge label="\u03bb" detail='\xE9' icon="\u{1F680}\uD83C\uDF0D" />;`;
  for (const fragment of [false, true]) {
    const result = transformSyntax(source, { rules: [rule], fragment });
    assert.equal(result.source, source);
    assert.deepEqual(result.steps, [{ name: 'UnStringEscape', edits: [] }]);
  }
});

test('UnStringEscape decodes JSX expression strings while preserving attribute and text spelling', () => {
  const source = String.raw`const badge = <Badge label="\u03bb" content={'\u03bb'}>{'\xE9'}<Part icon={'\u{1F680}\uD83C\uDF0D'} label='\xE9' />\u03bb</Badge>;`;
  const expected = String.raw`const badge = <Badge label="\u03bb" content={'λ'}>{'é'}<Part icon={'🚀🌍'} label='\xE9' />\u03bb</Badge>;`;
  for (const fragment of [false, true]) {
    assert.equal(run(source, { fragment }), expected);
  }
});
