import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import rule from '../scripts/syntax/rules/simplify-sequence.mjs';

const run = source => transformSyntax(source, { rules: [rule] }).source;

test('splits declarations on separate lines at the original indentation', () => {
  for (const kind of ['var', 'let', 'const']) {
    for (const [head, indent, tail] of [
      ['// before\n', '', '\n// after'],
      ['function read() {\n', '  ', '\n}'],
      ['function read() {\n  if (ready) {\n', '    ', '\n  }\n}'],
      ['\t{\n', '\t\t', '\n\t}'],
    ]) {
      const initial = 'build(() =>\n' + indent + '    schema({\n' + indent + '      value: 1,\n' + indent + '    }),\n' + indent + '  )';
      const before = `${head}${indent}${kind} first = ${initial},\n${indent}  second = "next", third = 3, fourth = 4, fifth = 5, sixth = 6; /* keep */${tail}`;
      const after = `${head}${indent}${kind} first = ${initial};\n${indent}${kind} second = "next";\n${indent}${kind} third = 3;\n${indent}${kind} fourth = 4;\n${indent}${kind} fifth = 5;\n${indent}${kind} sixth = 6; /* keep */${tail}`;
      assert.equal(run(before), after);
    }
  }
});

test('all statement extraction paths use the original line indentation', () => {
  for (const [before, statements] of [
    ['first(), second();', ['first();', 'second();']],
    ['return (trace(), value);', ['trace();', 'return value;']],
    ['throw (trace(), fault);', ['trace();', 'throw fault;']],
    ['var value=(trace(), read());', ['trace();', 'var value = read();']],
    ['(holder = read()).value = 1;', ['holder = read();', 'holder.value = 1;']],
    ['if (trace(), ready) work();', ['trace();', 'if (ready) work();']],
    ['switch (trace(), key) { case 1: first(), second(); }', ['trace();', 'switch (key) { case 1: first();\n    second(); }']],
    ['for (setup(); ready;) {}', ['setup();', 'for (; ready;) {}']],
    ['for (var first=read(), second=(trace(), next()); ready;) {}', ['var first=read();', 'trace();', 'for (var second = (next()); ready;) {}']],
    ['for (var key in (trace(), table)) {}', ['trace();', 'for (var key in table) {}']],
    ['for (item of (trace(), items)) {}', ['trace();', 'for (item of items) {}']],
    ['(yield value), finish();', ['yield value;', 'finish();']],
  ]) {
    assert.equal(run(`function* read() {\n  {\n    ${before} // tail\n  }\n}`),
      `function* read() {\n  {\n    ${statements.join('\n    ')} // tail\n  }\n}`, before);
  }
});

test('retains CRLF and bytes surrounding an inline replacement', () => {
  assert.equal(run('// before\r\n\t{ first(), second(); } // after\r\n'),
    '// before\r\n\t{ first();\r\n\tsecond(); } // after\r\n');
});

test('composes nested splits without changing multiline initializer text', () => {
  assert.equal(run('  var read = function () {\n    return (trace(), value);\n  }, next = build(\n    1,\n    2\n  );'),
    '  var read = function () {\n    trace();\n    return value;\n  };\n  var next = build(\n    1,\n    2\n  );');
});


test('formats split if branches and keeps their surrounding text', () => {
  assert.equal(run('  if (ready) first(), second(); else third(), fourth(); // keep'),
    '  if (ready) {\n    first();\n    second();\n  } else {\n    third();\n    fourth();\n  } // keep');
});
