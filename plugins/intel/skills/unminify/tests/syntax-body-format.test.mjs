import assert from 'node:assert/strict';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/simplify-sequence.mjs';

const run = source => transformSyntax(source, { rules: [rule] }).source;

test('new if and else blocks indent one inferred level and preserve surrounding bytes', () => {
  const source = '// keep\r\nfunction read() {\r\n    if (ready) first(), (state = next()); else third(), fourth(); // tail\r\n}\r\n';
  assert.equal(run(source), '// keep\r\nfunction read() {\r\n    if (ready) {\r\n        first();\r\n        state = next();\r\n    } else {\r\n        third();\r\n        fourth();\r\n    } // tail\r\n}\r\n');
});

test('new blocks in arrow and loop bodies infer tabs from their context', () => {
  const source = 'const read = () => {\n\twhile (ready) {\n\t\tif (pending) clear(), (pending = undefined);\n\t}\n};';
  assert.equal(run(source), 'const read = () => {\n\twhile (ready) {\n\t\tif (pending) {\n\t\t\tclear();\n\t\t\tpending = undefined;\n\t\t}\n\t}\n};');
});

test('multiline single-statement bodies align their closing brace with the if', () => {
  const source = 'function read() {\n  if (ready)\n    first(), second();\n}';
  assert.equal(run(source), 'function read() {\n  if (ready)\n    {\n    first();\n    second();\n  }\n}');
});

test('split operands discard safe wrapping parentheses and retain comments', () => {
  assert.equal(run('((state = next())), (/* keep */ state = undefined), ((finish()));'),
    'state = next();\n/* keep */ state = undefined;\nfinish();');
  assert.equal(run('( /* outer */ ( /* inner */ value ) ), finish();'),
    ' /* outer */  /* inner */ value  ;\nfinish();');
});

test('split operands keep statement-head and directive parentheses', () => {
  for (const operand of ['{ value: 1 }', 'function () {}', 'class {}', 'let [item]', '"use strict"']) {
    const source = `(${operand}), finish();`;
    assert.equal(run(source), `(${operand});\nfinish();`);
  }
});

test('removing safe parentheses preserves execution and template literal contents', () => {
  const source = 'const events = []; let value; function add(item) { events.push(item); return item; } if (true) (value = add(`first\nsecond`)), (value = add("last")); JSON.stringify({ value, events });';
  assert.equal(runInNewContext(run(source)), runInNewContext(source));
});

test('nested else-if extraction indents every generated block level', () => {
  assert.equal(run('  if (ready) first(), second(); else if (trace(), retry) third(), fourth();'),
    '  if (ready) {\n    first();\n    second();\n  } else {\n    trace();\n    if (retry) {\n      third();\n      fourth();\n    }\n  }');
});

test('let member and call operands discard parentheses unless followed by a bracket', () => {
  assert.equal(run('(let.value), (let(value)), (let + value), (let?.[value]), (let /* keep */ [value]), done();'),
    'let.value;\nlet(value);\nlet + value;\nlet?.[value];\n(let /* keep */ [value]);\ndone();');
  const source = 'var let = {value: 3}; var result; (result = let.value), (let.value); result;';
  assert.equal(runInNewContext(run(source)), runInNewContext(source));
});

test('indent inference does not mistake inline ancestor nesting for a whole indent unit', () => {
  const source = 'function read() { if (ready) {\n        if (pending) first(), second();\n} }\nfunction sibling() {\n    next();\n}';
  assert.equal(run(source), 'function read() { if (ready) {\n        if (pending) {\n            first();\n            second();\n        }\n} }\nfunction sibling() {\n    next();\n}');
});
