import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/simplify-sequence.mjs';
const run = source => transformSyntax(source, { rules: [rule] }).source;

test('literal contents do not determine statement indentation', () => {
  const source = 'function processQueue() {\n  const message = `header\n text`;\n  if (ready)\n    first(), second();\n}';
  assert.equal(run(source), 'function processQueue() {\n  const message = `header\n text`;\n  if (ready)\n    {\n    first();\n    second();\n  }\n}');
});

test('already indented else-if chains receive only the missing block level', () => {
  const source = 'function processQueue() {\n  if (active)\n    if (trace(), ready) first(), second();\n    else if (retry) third(), fourth();\n    else fifth(), sixth();\n}';
  assert.equal(run(source), 'function processQueue() {\n  if (active)\n    {\n    trace();\n    if (ready) {\n      first();\n      second();\n    }\n    else if (retry) {\n      third();\n      fourth();\n    }\n    else {\n      fifth();\n      sixth();\n    }\n  }\n}');
});

test('nested callbacks retain their existing indentation when a branch expands', () => {
  const source = 'function processQueue() {\n  if (ready)\n    (handler = () => {\n      if (pending) first(), second();\n    }),\n      subscribe(handler);\n}';
  assert.equal(run(source), 'function processQueue() {\n  if (ready)\n    {\n    handler = () => {\n      if (pending) {\n        first();\n        second();\n      }\n    };\n    subscribe(handler);\n  }\n}');
});
