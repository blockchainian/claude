import { countTokens, estimateInputTokens } from './prompt.mjs';
import { timed } from './timing.mjs';
import { isLexicalIdentifier, isModelBinding } from './identifiers.mjs';
import { fragmentFacts } from './fragments.mjs';
import { readFile, readdir, stat, realpath } from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { parse } from '@babel/parser';
import traverseModule from '@babel/traverse';
import generateModule from '@babel/generator';
import { createChunkIgnore } from './chunk-manifest.mjs';
import { createSyntaxPool } from './syntax/pool.mjs';
import { availableParallelism } from 'node:os';

const traverse = traverseModule.default ?? traverseModule;
const generate = generateModule.default ?? generateModule;
const exec = promisify(execFile);
const parseSource = source => timed('parse', () => parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true }));
const inside = (node, container) => node.start >= container.start && node.end <= container.end;
const isOwner = p => p.isFunction() || p.isClass();
// Batches keep their file analysis alive only while their plan is in use.
const batchAnalyses = new WeakMap();

/** Detach file analysis before a worker starts another file, even if batches survive. */
export function releasePlan(stages) {
  for (const stage of stages) for (const batch of stage) batchAnalyses.delete(batch);
}

function analyze(source, options) {
  const ast = parseSource(source);
  const bindings = new Map();
  let fragment;
  const ownerKey = node => `${node.type}:${node.start}:${node.end}`;
  const owners = new Map([[ownerKey(ast.program), ast.program]]);
  const contexts = new Map(), references = new WeakMap(), usedNames = new Set(), exports = new Map();
  traverse(ast, {
    Program(p) { ({ fragment } = fragmentFacts(p, options)); },
    enter(p) {
      if (!isOwner(p)) return;
      owners.set(ownerKey(p.node), p.node);
      let context = p.node;
      for (let parent = p.parentPath; parent; parent = parent.parentPath) {
        if (parent.isClass()) context = parent.node;
      }
      contexts.set(p.node, context);
    },
    Identifier(p) {
      usedNames.add(p.node.name);
      if (!isLexicalIdentifier(p)) return;
      const binding = p.scope.getBinding(p.node.name);
      if (binding) references.set(p.node, binding.identifier.start);
    },
    ExportNamedDeclaration(p) {
      if (p.node.declaration) exports.set(p.node.start,
        Object.values(p.get('declaration').getOuterBindingIdentifiers()).map(id => ({ key: id.start, name: id.name })));
    },
    Scope(p) {
      for (const binding of Object.values(p.scope.bindings)) bindings.set(binding.identifier.start, binding);
    },
  });
  return { source, ast, bindings, owners, contexts, references, usedNames, exports, fragment };
}

/** Selects a file or a directory's files; paths are relative to the enclosing Git root, ignore rules to their own file. */
export async function listFiles(target, { ignoreFile, onParse, root, ignores, pool } = {}) {
  const targets = await Promise.all((Array.isArray(target) ? target : [target]).map(file => realpath(file)));
  if (!targets.length) return [];
  if (!root) {
    const first = targets[0];
    const directory = (await stat(first)).isDirectory() ? first : path.dirname(first);
    root = (await exec('git', ['rev-parse', '--show-toplevel'], { cwd: directory })).stdout.trim();
  }
  ignores ??= await createChunkIgnore(root, ignoreFile);
  const candidates = new Set();
  function select(file) {
    if (/\.m?js$/.test(file) && !ignores(file)) candidates.add(file);
  }
  async function walk(folder) {
    for (const entry of (await readdir(folder, { withFileTypes: true })).sort((a,b)=>a.name.localeCompare(b.name))) {
      const file = path.join(folder, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== '.git' && !ignores(file + path.sep)) await walk(file);
      } else if (entry.isFile()) select(file);
    }
  }
  for (const file of targets) {
    if ((await stat(file)).isDirectory()) await walk(file);
    else select(file);
  }
  const ownPool = !pool;
  pool ??= createSyntaxPool();
  const files = [], pending = candidates.values();
  async function checkFiles() {
    for (const file of pending) {
      const source = await readFile(file, 'utf8');
      const startedAt = Date.now();
      const { parseable, elapsed } = await pool.run({ kind: 'parse', source });
      if (!parseable) continue;
      const relative = path.relative(root, file).split(path.sep).join('/');
      onParse?.(relative, startedAt, elapsed);
      files.push(relative);
    }
  }
  try {
    // Bound source buffers as well as ASTs; do not queue the entire corpus in memory.
    const results = await Promise.allSettled(Array.from({ length: Math.min(candidates.size, availableParallelism()) }, checkFiles));
    const failure = results.find(result => result.status === 'rejected');
    if (failure) throw failure.reason;
    return files.sort();
  } finally { if (ownPool) await pool.close(); }
}

function ownerChain(binding) {
  const chain = [];
  for (let p = binding.scope.path; p; p = p.parentPath) if (isOwner(p)) chain.unshift(p.node);
  return chain;
}

/** Binding keys and scope ranges are Babel start/end: UTF-16 indices, NOT bytes. */
export function planFile(source, { minSymbolLength = 4, maxInputTokens = 100000, fragment = false } = {}) {
  for (const [name, value] of Object.entries({ minSymbolLength, maxInputTokens })) {
    if (!Number.isSafeInteger(value) || value < 1) throw new RangeError(`${name} must be a positive integer`);
  }
  const analysis = analyze(source, { fragment });
  const { ast, bindings } = analysis;
  const remember = stages => {
    function fit(batch) {
      batchAnalyses.set(batch, analysis);
      if (estimateInputTokens(renderBatch(source, batch)) <= maxInputTokens || batch.keys.length === 1) return [batch];
      const middle = Math.ceil(batch.keys.length / 2);
      return [batch.keys.slice(0, middle), batch.keys.slice(middle)].flatMap(keys =>
        fit({ id: `${batch.id.split(':').slice(0, 4).join(':')}:${keys[0]}`, keys }));
    }
    return { stages: stages.map(stage => stage.flatMap(fit)) };
  };
  const candidates = [...bindings.values()].filter(b=>isModelBinding(b) && b.identifier.name.length < minSymbolLength
    && !(analysis.fragment && b.scope.path.isProgram()))
    .sort((a,b)=>a.identifier.start-b.identifier.start);
  if (!candidates.length) return { stages: [[]] };
  const whole = { id: `Program:${ast.program.start}:${ast.program.end}:${maxInputTokens}:${candidates[0].identifier.start}:file`,
    keys: candidates.map(b=>b.identifier.start) };
  batchAnalyses.set(whole, analysis);
  if (estimateInputTokens(renderBatch(source, whole)) <= maxInputTokens) return { stages: [[whole]] };
  const stages = [[], []];
  const groups = new Map();
  for (const b of candidates) {
    const owner = ownerChain(b)[0] ?? ast.program;
    if (!groups.has(owner)) groups.set(owner, []);
    groups.get(owner).push(b);
  }
  function emit(owner, items, stage) {
    if (!items.length) return;
    const size = countTokens(source.slice(owner.start,owner.end));
    if (owner !== ast.program && size <= maxInputTokens) {
      stages[stage].push({ id: `${owner.type}:${owner.start}:${owner.end}:${maxInputTokens}:${items[0].identifier.start}`, keys: items.map(b=>b.identifier.start) });
      return;
    }
    if (owner !== ast.program && size > maxInputTokens) {
      const local = [];
      const children = new Map();
      for (const b of items) {
        const chain = ownerChain(b);
        const child = chain[chain.indexOf(owner)+1];
        if (!child) local.push(b);
        else {
          if (!children.has(child)) children.set(child, []);
          children.get(child).push(b);
        }
      }
      if (children.size) {
        chunks(owner, local, stage);
        for (const [child, bs] of children) emit(child, bs, stage);
        return;
      }
    }
    chunks(owner, items, stage);
  }
  function chunks(owner, items, stage) {
    // When no nested function can be detached, use declaration-sized pieces.
    let chunk = [], used = 0, statements = new Set();
    function flush() {
      if (!chunk.length) return;
      stages[stage].push({ id: `${owner.type}:${owner.start}:${owner.end}:${maxInputTokens}:${chunk[0]}`, keys: chunk });
      chunk = []; used = 0; statements = new Set();
    }
    for (const b of items) {
      const statement = b.path.getStatementParent()?.node ?? b.path.node;
      const cost = statements.has(statement) ? countTokens(b.identifier.name) + 4 : countTokens(source.slice(statement.start,statement.end));
      if (chunk.length && (used + cost > maxInputTokens)) flush();
      chunk.push(b.identifier.start);
      statements.add(statement);
      used += cost;
    }
    flush();
  }
  for (const [owner, items] of groups) emit(owner, items, owner === ast.program ? 0 : 1);
  return remember(stages);
}

function printContext(node) {
  const code = generate(node, { comments: false }).code;
  if (['FunctionExpression','ArrowFunctionExpression','ClassExpression'].includes(node.type)) return `(${code});`;
  if (node.type === 'ObjectMethod') return `({${code}});`;
  if (['ClassMethod','ClassPrivateMethod'].includes(node.type)) return `class Context { ${code} }`;
  return code;
}

export function renderBatch(source, batch, renames = new Map(), {
  placeholderOffset = 0, placeholderCount = batch.keys.length,
} = {}) {
  const analysis = batchAnalyses.get(batch) ?? analyze(source);
  if (analysis.source !== source) throw new Error('Batch source changed');
  const { bindings, owners, contexts, references, usedNames, exports } = analysis;
  const [type, rawStart, rawEnd, rawBudget] = batch.id.split(':');
  const start = Number(rawStart), end = Number(rawEnd), budget = Number(rawBudget);
  const owner = owners.get(`${type}:${start}:${end}`);
  if (!owner || owner.end !== end || !Number.isSafeInteger(budget) || budget < 1) throw new Error('Invalid batch range');
  // Private names and super require their actual enclosing class in a detached batch.
  const context = contexts.get(owner) ?? owner;
  const placeholders = {};
  const targets = new Map(renames);
  for (const key of targets.keys()) if (!bindings.has(key)) throw new Error(`Unknown binding key: ${key}`);
  for (const [index, key] of batch.keys.entries()) {
    if (!bindings.has(key)) throw new Error(`Unknown binding key: ${key}`);
    const placeholder = `v${placeholderOffset + index + 1}`;
    placeholders[placeholder] = key;
    targets.set(key, placeholder);
  }
  // Reserve placeholder names even when the source already has a binding named v1.
  const reserved = name => /^v[1-9]\d*$/.test(name) && Number(name.slice(1)) <= placeholderCount;
  const batchKeys = new Set(batch.keys), finalNames = new Set(targets.values());
  const contextNames = new Map();
  let suffix = 0;
  function nameFor(key) {
    const name = targets.get(key) ?? bindings.get(key).identifier.name;
    if (batchKeys.has(key) || !reserved(name)) return name;
    if (!contextNames.has(key)) {
      let replacement;
      do { replacement = `_context${++suffix}`; } while (usedNames.has(replacement) || finalNames.has(replacement));
      contextNames.set(key, replacement);
    }
    return contextNames.get(key);
  }
  const selected = batch.keys.map(key => bindings.get(key).identifier);
  const containsKey = node => selected.some(id => inside(id, node));
  const wholeFile = batch.id.endsWith(':file');
  // Copy syntax only from the selected context. Cached scopes and nodes stay immutable.
  function copy(node) {
    const result = { ...node };
    // A detached function keeps its declaration header, so sibling context stays legal.
    const stub = !wholeFile && owner.type !== 'Program' && node !== owner && node !== context
      && owners.has(`${node.type}:${node.start}:${node.end}`) && !containsKey(node);
    for (const [key, value] of Object.entries(node)) {
      if (stub && key === 'body') {
        if (node.type.startsWith('Class')) result.body = { ...value, body: [] };
        else if (value.type === 'BlockStatement') result.body = { ...value, body: [] };
        else result.body = { type: 'UnaryExpression', operator: 'void', prefix: true, argument: { type: 'NumericLiteral', value: 0 } };
      } else if (Array.isArray(value)) {
        const items = !wholeFile && node.type === 'Program' && key === 'body'
          ? value.filter(item => item.type.startsWith('Import') || containsKey(item)) : value;
        result[key] = items.map(item => item?.type ? copy(item) : item);
      } else if (value?.type) result[key] = copy(value);
    }
    const bindingKey = references.get(node);
    if (bindingKey !== undefined) result.name = nameFor(bindingKey);
    if (result.type === 'ObjectProperty' && result.shorthand) {
      const value = result.value.type === 'AssignmentPattern' ? result.value.left : result.value;
      if (result.key.name !== value.name) {
        result.shorthand = false;
        if (result.extra) result.extra = { ...result.extra, shorthand: false };
      }
    }
    if (result.type === 'Program') result.body = result.body.flatMap(statement => {
      const exported = exports.get(statement.start);
      if (!exported || !exported.some(({ key, name }) => nameFor(key) !== name)) return [statement];
      return [statement.declaration, { type: 'ExportNamedDeclaration', declaration: null, source: null,
        specifiers: exported.map(({ key, name }) => ({ type: 'ExportSpecifier',
          local: { type: 'Identifier', name: nameFor(key) }, exported: { type: 'Identifier', name } })) }];
    });
    return result;
  }
  const contextOwner = copy(context);
  if (wholeFile) return { context: printContext(contextOwner), placeholders };
  const ast = { type: 'File', program: contextOwner.type === 'Program' ? contextOwner
    : { type: 'Program', sourceType: 'module', body: [contextOwner] } };
  const fits = () => estimateInputTokens({ context: printContext(contextOwner), placeholders }) <= budget;
  if (owner.type === 'Program') {
    // Shorten the longest optional body with logarithmically many tokenizations.
    const bodies = [];
    traverse(ast, { noScope: true, Function(p) {
      if (p.node.body.type === 'BlockStatement' && !containsKey(p.node.body)) bodies.push(p.node.body);
    } });
    bodies.sort((a, b) => (b.end - b.start) - (a.end - a.start));
    for (const body of bodies) {
      if (fits()) break;
      const statements = body.body;
      let low = 0, high = statements.length;
      while (low < high) {
        const middle = Math.ceil((low + high) / 2);
        body.body = statements.slice(0, middle);
        if (fits()) low = middle;
        else high = middle - 1;
      }
      body.body = statements.slice(0, low);
    }
  }
  if (!fits()) {
    // Retain the declaration heads if a flat body cannot fit the token budget.
    traverse(ast, {
      noScope: true,
      BlockStatement(p) {
        if (inside(p.node,contextOwner)) p.node.body = p.node.body.filter(containsKey);
      },
      VariableDeclaration(p) {
        if (inside(p.node,contextOwner) && containsKey(p.node)) {
          p.node.declarations = p.node.declarations.filter(containsKey);
        }
      },
      VariableDeclarator(p) {
        if (inside(p.node,contextOwner) && p.node.init && !containsKey(p.node.init)) {
          p.node.init = { type:'UnaryExpression',operator:'void',prefix:true,argument:{type:'NumericLiteral',value:0} };
        }
      },
    });
  }
  return { context: printContext(contextOwner), placeholders };
}
