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
const parseSource = source => parse(source, { sourceType: 'unambiguous' });
const bytes = code => Buffer.byteLength(code, 'utf8');
const inside = (node, container) => node.start >= container.start && node.end <= container.end;
const isOwner = p => p.isFunction() || p.isClass();

function analyze(source) {
  const ast = parseSource(source);
  const bindings = new Map();
  const ownerKey = node => `${node.type}:${node.start}:${node.end}`;
  const owners = new Map([[ownerKey(ast.program), ast.program]]);
  traverse(ast, {
    enter(p) { if (isOwner(p)) owners.set(ownerKey(p.node), p.node); },
    Scope(p) {
      for (const binding of Object.values(p.scope.bindings)) bindings.set(binding.identifier.start, binding);
    },
  });
  return { ast, bindings, owners };
}

/** Paths are relative to the enclosing Git root; ignore rules to their own file. */
export async function listFiles(dir, { ignoreFile } = {}) {
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
        try { parseSource(source); }
        catch (error) { if (error instanceof SyntaxError) continue; throw error; }
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
  const { ast, bindings } = analyze(source);
  const candidates = [...bindings.values()].filter(b=>b.kind !== 'module' && b.identifier.name.length <= shortLen)
    .sort((a,b)=>a.identifier.start-b.identifier.start);
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
  return { stages };
}

function printContext(node) {
  const code = generate(node, { comments: false }).code;
  if (['FunctionExpression','ArrowFunctionExpression','ClassExpression'].includes(node.type)) return `(${code});`;
  if (node.type === 'ObjectMethod') return `({${code}});`;
  if (['ClassMethod','ClassPrivateMethod'].includes(node.type)) return `class Context { ${code} }`;
  return code;
}

export function renderBatch(source, batch, renames = new Map()) {
  const { ast, bindings, owners } = analyze(source);
  const [type, rawStart, rawEnd, rawBudget] = batch.id.split(':');
  const start = Number(rawStart), end = Number(rawEnd), budget = Number(rawBudget);
  const owner = owners.get(`${type}:${start}:${end}`);
  if (!owner || owner.end !== end || !Number.isSafeInteger(budget) || budget < 1) throw new Error('Invalid batch range');
  let contextOwner = owner;
  // Private names and super require their actual enclosing class in a detached batch.
  traverse(ast, { enter(p) {
    if (p.node !== owner) return;
    for (let parent = p.parentPath; parent; parent = parent.parentPath) {
      if (parent.isClass()) contextOwner = parent.node;
    }
  } });
  const placeholders = {};
  const targets = new Map(renames);
  for (const [index, key] of batch.keys.entries()) {
    if (!bindings.has(key)) throw new Error(`Unknown binding key: ${key}`);
    placeholders[`v${index+1}`] = key;
    targets.set(key, `v${index+1}`);
  }
  // Reserve placeholder names even when the source already has a binding named v1.
  const reserved = new Set(Object.keys(placeholders));
  const batchKeys = new Set(batch.keys);
  for (const [key, b] of bindings) if (!batchKeys.has(key) && reserved.has(targets.get(key) ?? b.identifier.name)) {
    targets.set(key, b.scope.generateUidIdentifier('context').name);
  }
  const finalNames = new Set(targets.values());
  const pending = [];
  for (const [key, name] of targets) {
    const b = bindings.get(key);
    if (!b) throw new Error(`Unknown binding key: ${key}`);
    let temp;
    do { temp = b.scope.generateUidIdentifier('binding').name; } while (finalNames.has(temp));
    b.scope.rename(b.identifier.name, temp);
    pending.push([b.scope, temp, name]);
  }
  for (const [scope, temp, name] of pending) scope.rename(temp, name);

  const selected = batch.keys.map(key=>bindings.get(key).identifier);
  const containsKey = node => selected.some(id=>inside(id,node));
  // A detached function keeps its declaration header, so sibling context stays legal.
  traverse(ast, { enter(p) {
    if (!isOwner(p) || p.node === owner || !inside(p.node,contextOwner)) return;
    if (owner.type === 'Program' || (!containsKey(p.node) && p.node !== contextOwner)) {
      if (p.isClass()) p.node.body.body = [];
      else if (p.node.body.type === 'BlockStatement') p.node.body.body = [];
      else p.node.body = { type:'UnaryExpression',operator:'void',prefix:true,argument:{type:'NumericLiteral',value:0} };
      p.skip();
    }
  } });
  if (owner.type === 'Program') {
    owner.body = owner.body.filter(node=>node.type.startsWith('Import') || containsKey(node));
  }
  if (bytes(printContext(contextOwner)) > budget) {
    // Retain the declaration heads if a flat body cannot fit the byte target.
    traverse(ast, {
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
