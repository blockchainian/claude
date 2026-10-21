import { createRequire } from 'node:module';
import { readFileSync, lstatSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { availableParallelism } from 'node:os';
import { parse } from '@babel/parser';
import traverseModule from '@babel/traverse';

const require = createRequire(import.meta.url);
const traverse = traverseModule.default ?? traverseModule;
const environmentGlobals = new Set([
  ...require('@babel/helper-globals/data/browser-upper.json'),
  ...('arguments require module exports __dirname __filename global process Buffer Bun Deno '
    + 'window document navigator self location history screen console performance crypto '
    + 'fetch atob btoa setTimeout clearTimeout setInterval clearInterval setImmediate clearImmediate '
    + 'queueMicrotask requestAnimationFrame cancelAnimationFrame requestIdleCallback cancelIdleCallback '
    + 'addEventListener removeEventListener dispatchEvent postMessage importScripts close '
    + 'alert confirm prompt matchMedia getComputedStyle localStorage sessionStorage indexedDB '
    + 'caches customElements define').split(' '),
]);

/** Source alone cannot distinguish an unused script declaration from a slice. */
export function fragmentFacts(program, { fragment = false } = {}) {
  const bindings = Object.values(program.scope.bindings);
  const exports = program.node.body.filter(node => node.type.startsWith('Export'));
  const unboundExports = new Set();
  for (const node of exports) {
    if (node.type !== 'ExportNamedDeclaration' || node.source) continue;
    for (const specifier of node.specifiers) {
      if (specifier.type === 'ExportSpecifier' && !program.scope.getBinding(specifier.local.name)) {
        unboundExports.add(specifier.local.name);
      }
    }
  }
  return { fragment: fragment || unboundExports.size > 0, unboundExports, topLevelNames: bindings
    .sort((a, b) => a.identifier.start - b.identifier.start).map(binding => binding.identifier.name) };
}

/** Index only names and flags, releasing each file's AST before reading another. */
export function indexFragments(files) {
  return indexFragmentContext(files).fragments;
}

/** Return cloneable binding evidence without retaining the file's syntax tree. */
export function summarizeFragmentSource(source) {
  let ast;
  try { ast = parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true, plugins: ['jsx'] }); }
  catch (error) { if (error instanceof SyntaxError) return null; throw error; }
  let summary;
  traverse(ast, { Program(program) {
    const free = Object.keys(program.scope.globals);
    const unknown = free.filter(name => !environmentGlobals.has(name) && !program.scope.hasBinding(name));
    const bindings = Object.values(program.scope.bindings);
    const topLevelBindings = bindings.map(binding => binding.identifier.name);
    const unused = bindings.filter(binding => !binding.referenced).map(binding => binding.identifier.name);
    const declarations = bindings.filter(binding => binding.kind !== 'module').map(binding => binding.identifier.name);
    const aliases = bindings.filter(binding => binding.kind === 'module' && !binding.referenced).map(binding => binding.identifier.name);
    const boundary = program.node.body.some(node => node.type.startsWith('Export'))
      || free.some(name => ['require', 'module', 'exports'].includes(name));
    const facts = fragmentFacts(program);
    summary = { topLevelBindings, declarations, aliases, free,
      facts: { ...facts, unboundExports: [...facts.unboundExports], unknown, unused, boundary } };
    program.stop();
  } });
  return summary;
}

/** Skip the same missing paths and non-files in serial and worker indexing. */
export function summarizeFragmentFile(path) {
  try {
    return lstatSync(path).isFile() ? summarizeFragmentSource(readFileSync(path, 'utf8')) : null;
  } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}

/** Merge file summaries in input order so every Set and Map keeps serial ordering. */
function createFragmentIndex() {
  const topLevelBindings = new Set();
  const summaries = new Map(), readers = new Map(), declarations = new Map(), aliases = new Map();
  function add(file, summary) {
    if (!summary) return;
    for (const name of summary.topLevelBindings) topLevelBindings.add(name);
    for (const [index, names] of [[declarations, summary.declarations], [aliases, summary.aliases], [readers, summary.free]]) {
      for (const name of names) {
        if (!index.has(name)) index.set(name, new Set());
        index.get(name).add(file);
      }
    }
    summaries.set(file, summary.facts);
  }
  function finish() {
    const collisions = new Set();
    for (const [name, importers] of aliases) {
      for (const importer of importers) for (const declarer of declarations.get(name) ?? []) {
        if (importer !== declarer) { collisions.add(importer); collisions.add(declarer); }
      }
    }
    const fragments = new Set();
    for (const [file, facts] of summaries) {
      // Explicit module boundaries also occur in sliced files with missing exports.
      // Otherwise CJS/UMD contracts take precedence over accidental short-name
      // matches between unrelated packages in the same repository.
      const shared = facts.unused.some(name => [...(readers.get(name) ?? [])].some(reader => reader !== file));
      if (facts.fragment || (!facts.boundary && (facts.unknown.length > 0 || shared || collisions.has(file)))) fragments.add(file);
    }
    return { fragments, topLevelBindings };
  }
  return { add, finish };
}

/** Preserve the program-binding evidence used to authorize fragment globals. */
export function indexFragmentContext(files) {
  const index = createFragmentIndex();
  for (const [file, source] of files) index.add(file, summarizeFragmentSource(source));
  return index.finish();
}

/** Git-root scope keeps sibling slices visible across directory-sized runs. */
export function loadFragmentIndex(root, inputs) {
  return loadFragmentContext(root, inputs).fragments;
}

/** Use the same Git-root/input scope for fragment flags and binding evidence. */
export function loadFragmentContext(root, inputs, pool) {
  let files;
  try {
    files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'],
      { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] }).split('\0').filter(Boolean);
  } catch (error) {
    if (!String(error.stderr).includes('not a git repository')) throw error;
    files = inputs;
  }
  const paths = [...new Set([...files, ...inputs])].filter(file => /\.[cm]?js$/.test(file));
  if (pool) return loadParallelFragmentContext(root, paths, pool);
  const index = createFragmentIndex();
  for (const file of paths) index.add(file, summarizeFragmentFile(join(root, file)));
  return index.finish();
}

async function loadParallelFragmentContext(root, files, pool) {
  const index = createFragmentIndex(), width = availableParallelism();
  // Queue paths only, one bounded window at a time; workers read their own source.
  for (let offset = 0; offset < files.length; offset += width) {
    const batch = files.slice(offset, offset + width);
    const results = await Promise.allSettled(batch.map(file => pool.run({ kind: 'fragment', path: join(root, file) })));
    const failure = results.find(result => result.status === 'rejected');
    if (failure) throw failure.reason;
    for (const [position, file] of batch.entries()) index.add(file, results[position].value);
  }
  return index.finish();
}
