// Ported from wakaru v1.14.0 (Apache-2.0); independently authored test inputs.
import assert from 'node:assert/strict';
import test from 'node:test';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { indexFragmentContext } from '../scripts/fragments.mjs';
import rule from '../scripts/syntax/rules/flip-comparisons.mjs';

const run = (source, options = {}) => transformSyntax(source, { rules: [rule], ...options }).source;

test('FlipComparisons preserves equality operators and reverses all relational operators', () => {
  for (const [operator, flipped] of [['==', '=='], ['!=', '!='], ['===', '==='], ['!==', '!=='], ['<', '>'], ['>', '<'], ['<=', '>='], ['>=', '<=']]) {
    assert.equal(run(`check(37 ${operator} count);`), `check(count ${flipped} 37);`);
  }
});

test('FlipComparisons recognizes every literal-like branch', () => {
  for (const value of ['null', 'true', 'false', '4.25', '0x2a', '"ready"', '91n', '`plain`', 'void 17', '!0', '!1', '!0x1', '-8', '-19n', 'undefined', 'NaN', 'Infinity', '-Infinity']) {
    assert.equal(run(`check(${value} === read());`), `check(read() === ${value});`);
  }
});

test('FlipComparisons retains pairs of constants, nonconstants and unsupported operators byte for byte', () => {
  for (const expression of ['7 < 9', 'null == false', 'undefined === Infinity', 'left > right', '/x/ === value', '`a${item}` === value', 'void item === value', '!2 === value', '+7 === value', '-NaN === value', '-undefined === value', '~0 === value', '(7) === value', 'void (7) === value', '!(0) === value', '-(8) === value', '-(Infinity) === value', '7 in object', '7 instanceof Type', '7 + value', '7 / value', '7 & value', '7 && value', '7 ?? value']) {
    const source = `/* exact */ check( ${expression} );\r\n`;
    assert.equal(run(source), source);
  }
});

test('FlipComparisons respects local, imported and program constant bindings', () => {
  for (const name of ['undefined', 'NaN', 'Infinity']) {
    for (const source of [`let ${name}; check(${name} < value);`, `function inspect(${name}) { return ${name} === value; }`, `import { value as ${name} } from "pkg"; check(${name} == value);`, `try {} catch (${name}) { check(${name} != value); }`]) assert.equal(run(source), source);
    assert.equal(run(`function inspect(${name}) {} check(${name} == value);`), `function inspect(${name}) {} check(value == ${name});`);
    assert.equal(run(`function inspect(${name}) { return 3 === ${name}; }`), `function inspect(${name}) { return ${name} === 3; }`);
  }
  assert.equal(run('function inspect(Infinity) { return -Infinity === value; }'), 'function inspect(Infinity) { return -Infinity === value; }');
});

test('FlipComparisons blocks global constant reads on either side in dynamic scopes', () => {
  for (const dynamic of ['with (record) {}', 'eval(code);', 'eval("work()");', '(eval)();', 'function local(eval) { eval("x"); }']) {
    for (const constant of ['undefined', 'NaN', 'Infinity', '-Infinity']) {
      for (const expression of [`${constant} < value`, `2 === ${constant}`]) {
        const source = `${dynamic} check(${expression});`;
        assert.equal(run(source), source);
      }
    }
    assert.equal(run(`${dynamic} check(2 <= value);`), `${dynamic} check(value >= 2);`);
  }
});

test('FlipComparisons permits indirect eval and unbound CommonJS require reads', () => {
  for (const expression of ['(0, eval)(code)', 'eval("require")', 'eval(" require ( \'os\' ) . platform ; ")', 'eval("quire".replace(/^/, "re"))', 'eval?.(code)']) {
    assert.equal(run(`${expression}; check(NaN !== value);`), `${expression}; check(value !== NaN);`);
  }
  const source = 'const require = loader; eval("require"); check(Infinity < value);';
  assert.equal(run(source), source);
});

test('FlipComparisons matches the require-eval argument traversal boundary', () => {
  assert.equal(run('eval("require", eval("work()")); check(Infinity === value);'), 'eval("require", eval("work()")); check(value === Infinity);');
  const source = 'eval("require", function require() {}); check(Infinity === value);';
  assert.equal(run(source), source);
  assert.equal(run('eval("require", 2 < value); check(Infinity === value);'), 'eval("require", value > 2); check(value === Infinity);');
});

test('FlipComparisons keeps compact return and throw tokens separate', () => {
  assert.equal(run('function inspect(){return!0===value;} function fail(){throw!1<value;}'), 'function inspect(){return value===!0;} function fail(){throw value>!1;}');
});

test('FlipComparisons retains fragment global constant reads without visitor-level binding proof', () => {
  for (const name of ['undefined', 'NaN', 'Infinity']) {
    const source = `var shared = ${name} === value; export { missing };`;
    assert.equal(run(source, { fragment: true }), source);
    const shadowed = indexFragmentContext([['piece.js', source], ['binding.js', `let ${name};`]]);
    assert.equal(run(source, { fragment: true, fragmentContext: shadowed }), source);
    const clear = indexFragmentContext([['piece.js', source], ['binding.js', 'let unrelated;']]);
    assert.equal(run(source, { fragment: true, fragmentContext: clear }), source);
    assert.equal(run(`var shared = 5 === ${name};`, { fragment: true, fragmentContext: clear }), `var shared = 5 === ${name};`);
  }
  assert.equal(run('var shared = -Infinity < value;', { fragment: true }), 'var shared = -Infinity < value;');
});

test('FlipComparisons preserves fragment declarations while flipping literal comparisons', () => {
  const source = 'var first = 6 < limit, second; let third = null === value; const fourth = 9 >= size; export { missing };';
  assert.equal(run(source, { fragment: true }), 'var first = limit > 6, second; let third = value === null; const fourth = size <= 9; export { missing };');
});

test('FlipComparisons composes nested edits and preserves parentheses and comment trivia', () => {
  assert.equal(run('check(3 === (4 < size), (void (8)) !== call());'), 'check((size > 4) === 3, (void (8)) !== call());');
  assert.equal(run('// unchanged\r\ncheck( 3 /* < */ <= /* >= */ read(5 === value) );\r\n'), '// unchanged\r\ncheck( read(value === 5) /* < */ >= /* >= */ 3 );\r\n');
  assert.equal(run('check(delete cache[3 === value]);'), 'check(delete cache[value === 3]);');
});

test('FlipComparisons keeps expression statements valid when moving object, function and class operands', () => {
  for (const expression of ['{}', 'function () {}', 'class {}', '{}.size', 'function () {}()', '{}.size + delta', '{}.size++']) {
    assert.equal(run(`3 === ${expression};`), `(${expression}) === 3;`);
    assert.equal(run(`3 === ${expression} === result;`), `(${expression}) === 3 === result;`);
  }
});
