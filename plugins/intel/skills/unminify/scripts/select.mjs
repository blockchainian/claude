import { timed } from './timing.mjs';
import { isLexicalIdentifier } from './identifiers.mjs';
import { readFile, readdir, stat, realpath } from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { parse } from '@babel/parser';
import traverseModule from '@babel/traverse';
import generateModule from '@babel/generator';
import ignore from 'ignore';

const traverse = traverseModule.default ?? traverseModule;
const generate = generateModule.default ?? generateModule;
const exec = promisify(execFile);
const parseSource = source => timed('parse', () => parse(source, { sourceType: 'unambiguous' }));
const bytes = code => Buffer.byteLength(code, 'utf8');
const inside = (node, container) => node.start >= container.start && node.end <= container.end;
const isOwner = p => p.isFunction() || p.isClass();
// Batches keep their file analysis alive only while their plan is in use.
const batchAnalyses = new WeakMap();

function analyze(source) {
  const ast = parseSource(source);
  const bindings = new Map();
  const ownerKey = node => `${node.type}:${node.start}:${node.end}`;
  const owners = new Map([[ownerKey(ast.program), ast.program]]);
  const contexts = new Map(), references = new WeakMap(), usedNames = new Set(), exports = new Map();
  traverse(ast, {
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
  return { source, ast, bindings, owners, contexts, references, usedNames, exports };
}

/** Paths are relative to the enclosing Git root; ignore rules to their own file. */
export async function listFiles(dir, { ignoreFile, onParse } = {}) {
  dir = await realpath(dir);
  const root = (await exec('git', ['rev-parse', '--show-toplevel'], { cwd: dir })).stdout.trim();
  const rulesFile = ignoreFile ? await realpath(ignoreFile) : path.join(root, '.ignore');
  let rules = '';
  try { rules = await readFile(rulesFile, 'utf8'); }
  catch (error) { if (error.code !== 'ENOENT' || ignoreFile) throw error; }
  const matcher = ignore().add(rules);
  const ignored = file => {
    const relative = path.relative(path.dirname(rulesFile), file).split(path.sep).join('/');
    return relative !== '' && !relative.startsWith('../') && matcher.ignores(relative);
  };
  const files = [];
  async function walk(folder) {
    for (const entry of (await readdir(folder, { withFileTypes: true })).sort((a,b)=>a.name.localeCompare(b.name))) {
      const file = path.join(folder, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== '.git' && !ignored(file + path.sep)) await walk(file);
      } else if (entry.isFile() && /\.m?js$/.test(entry.name) && !ignored(file)) {
        const source = await readFile(file, 'utf8');
        const startedAt = Date.now(), started = performance.now();
        try { parseSource(source); }
        catch (error) { if (error instanceof SyntaxError) continue; throw error; }
        onParse?.(path.relative(root, file).split(path.sep).join('/'), startedAt, performance.now() - started);
        files.push(path.relative(root,file).split(path.sep).join('/'));
      }
    }
  }
  if (!(await stat(dir)).isDirectory()) throw new TypeError('Expected a directory');
  await walk(dir);
  return files.sort();
}

function ownerChain(binding) {
  const chain = [];
  for (let p = binding.scope.path; p; p = p.parentPath) if (isOwner(p)) chain.unshift(p.node);
  return chain;
}

/** Binding keys and scope ranges are Babel start/end: UTF-16 indices, NOT bytes. */
export function planFile(source, { shortLen = 3, maxIds = 60, contextBytes = 8000 } = {}) {
  for (const [name, value] of Object.entries({ shortLen, maxIds, contextBytes })) {
    if (!Number.isSafeInteger(value) || value < 1) throw new RangeError(`${name} must be a positive integer`);
  }
  const analysis = analyze(source);
  const { ast, bindings } = analysis;
  const remember = stages => {
    for (const batch of stages.flat()) batchAnalyses.set(batch, analysis);
    return { stages };
  };
  const candidates = [...bindings.values()].filter(b=>b.kind !== 'module' && b.identifier.name.length <= shortLen)
    .sort((a,b)=>a.identifier.start-b.identifier.start);
  if (bytes(source) <= contextBytes && candidates.length <= maxIds) {
    return remember([candidates.length ? [{
      id: `Program:${ast.program.start}:${ast.program.end}:${contextBytes}:${candidates[0].identifier.start}:file`,
      keys: candidates.map(b=>b.identifier.start),
    }] : []]);
  }
  const stages = [[], []];
  const groups = new Map();
  for (const b of candidates) {
    const owner = ownerChain(b)[0] ?? ast.program;
    if (!groups.has(owner)) groups.set(owner, []);
    groups.get(owner).push(b);
  }
  function emit(owner, items, stage) {
    if (!items.length) return;
    const size = bytes(source.slice(owner.start,owner.end));
    if (owner !== ast.program && size <= contextBytes && items.length <= maxIds) {
      stages[stage].push({ id: `${owner.type}:${owner.start}:${owner.end}:${contextBytes}:${items[0].identifier.start}`, keys: items.map(b=>b.identifier.start) });
      return;
    }
    if (owner !== ast.program && (size > contextBytes || items.length > maxIds)) {
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
      stages[stage].push({ id: `${owner.type}:${owner.start}:${owner.end}:${contextBytes}:${chunk[0]}`, keys: chunk });
      chunk = []; used = 0; statements = new Set();
    }
    for (const b of items) {
      const statement = b.path.getStatementParent()?.node ?? b.path.node;
      const cost = statements.has(statement) ? b.identifier.name.length + 12 : bytes(source.slice(statement.start,statement.end));
      if (chunk.length && (chunk.length >= maxIds || used + cost > contextBytes)) flush();
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
  if (owner.type === 'Program') {
    // Preserve implementation prefixes, shortening the longest function body first.
    const bodies = [];
    traverse(ast, { noScope: true, Function(p) {
      if (p.node.body.type === 'BlockStatement' && !containsKey(p.node.body)) bodies.push(p.node.body);
    } });
    while (bytes(printContext(contextOwner)) > budget) {
      const longest = bodies.filter(body=>body.body.length)
        .sort((a,b)=>bytes(printContext(b))-bytes(printContext(a)))[0];
      if (!longest) break;
      longest.body.pop();
    }
  }
  if (bytes(printContext(contextOwner)) > budget) {
    // Retain the declaration heads if a flat body cannot fit the byte target.
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
