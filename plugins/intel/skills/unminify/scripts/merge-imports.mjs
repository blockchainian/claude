import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import traverseModule from '@babel/traverse';
import { analyzeChunk } from './chunks.mjs';
import { isLexicalReference } from './identifiers.mjs';
import { openJournal, atomicWrite } from './journal.mjs';
const traverse = traverseModule.default ?? traverseModule;
export const importMergeVersion = 1;
const importedName = spec => spec.type === 'ImportSpecifier' ? spec.imported.name ?? spec.imported.value
  : spec.type === 'ImportDefaultSpecifier' ? 'default' : '*';
const importIdentity = (node, spec) => [node.source.value, importedName(spec), spec.type === 'ImportNamespaceSpecifier'];
const external = node => node.type === 'ImportDeclaration' && !node.source.value.startsWith('.');

function ordered(sources) {
  return [...sources].sort(([, a], [, b]) => {
    const line = source => Number(/^\/\/ Recovered from .+ lines (\d+)-\d+/.exec(source)?.[1] ?? 0);
    return line(a) - line(b);
  });
}
function plan(sources, { selected = new Set(sources.keys()), ignored = new Set(), chunkSiblings = false } = {}) {
  const analysis = analyzeChunk(sources), groups = new Map(), removed = new Map();
  if (analysis.statistics.dynamicScope) return { analysis, removed };
  for (const [file] of ordered(sources)) for (const node of analysis.files.get(file).ast.program.body) {
    if (!external(node)) continue;
    // Attributes and phases affect module identity; keep these declarations intact.
    if (node.attributes?.length || node.assertions?.length || node.phase) continue;
    for (const spec of node.specifiers) {
      const key = JSON.stringify(importIdentity(node, spec));
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push({ file, node, spec, name: spec.local.name });
    }
  }
  for (const members of groups.values()) {
    if (members.length < 2) continue;
    const [kept, ...duplicates] = members;
    if (members.some(member => analysis.bindings.get(member.name).declarations.length !== 1)) continue;
    const touched = new Set(members.flatMap(member => [...analysis.bindings.get(member.name).files]));
    if ([...touched].some(file => ignored.has(file) || !chunkSiblings && !selected.has(file))) continue;
    const captured = duplicates.some(member => analysis.bindings.get(member.name).references.some(({ path }) => {
      const binding = path.scope.getBinding(kept.name);
      return binding && binding.identifier !== kept.spec.local;
    }));
    if (captured) continue;
    for (const member of duplicates) removed.set(member.name, { ...member, kept });
  }
  return { analysis, removed };
}
function rewrite(sources, { analysis, removed }) {
  const output = new Map();
  for (const [file, source] of sources) {
    const edits = [];
    for (const member of removed.values()) for (const ref of analysis.bindings.get(member.name).references) {
      if (ref.file !== file) continue;
      const path = ref.path, node = path.node, parent = path.parentPath;
      let text = member.kept.name;
      if (parent.isObjectProperty() && parent.node.shorthand && path.key === 'value') text = `${source.slice(node.start, node.end)}: ${text}`;
      else if (parent.isExportSpecifier() && parent.node.exported.start === node.start) text = `${text} as ${source.slice(node.start, node.end)}`;
      edits.push({ start: node.start, end: node.end, text });
    }
    for (const node of analysis.files.get(file).ast.program.body) {
      if (!external(node) || !node.specifiers.some(spec => removed.has(spec.local.name))) continue;
      const kept = node.specifiers.filter(spec => !removed.has(spec.local.name));
      let text = '';
      if (kept.length) {
        const names = kept.filter(spec => spec.type === 'ImportSpecifier');
        const parts = kept.filter(spec => spec.type !== 'ImportSpecifier').map(spec => source.slice(spec.start, spec.end));
        if (names.length) parts.push(`{${names.map(spec => source.slice(spec.start, spec.end)).join(', ')}}`);
        text = `import ${parts.join(', ')} from ${source.slice(node.source.start, node.end)}`;
      }
      edits.push({ start: node.start, end: node.end, text });
    }
    let cursor = 0, text = '';
    for (const edit of edits.sort((a, b) => a.start - b.start)) {
      if (edit.start < cursor) throw new Error('Overlapping import merge edits');
      text += source.slice(cursor, edit.start) + edit.text;
      cursor = edit.end;
    }
    output.set(file, text + source.slice(cursor));
  }
  return output;
}

// Resolve each use independently of the edit plan, including sibling free references.
function facts(sources) {
  const analysis = analyzeChunk(sources), owners = new Map(), identities = new Map(), bindings = [], references = [], loads = [];
  for (const [file, record] of analysis.files) {
    const all = new Set();
    traverse(record.ast, { Scope(path) { Object.values(path.scope.bindings).forEach(binding => all.add(binding)); } });
    let index = 0;
    for (const binding of [...all].sort((a, b) => a.identifier.start - b.identifier.start)) {
      const spec = binding.path.node, declaration = binding.path.parent;
      let identity;
      if (binding.kind === 'module' && external(declaration)) identity = ['external', ...importIdentity(declaration, spec)];
      else {
        identity = ['binding', file, index++];
        bindings.push([identity, binding.identifier.name, binding.kind]);
      }
      identities.set(binding, identity);
      if (binding.scope === record.program.scope) {
        if (!owners.has(binding.identifier.name)) owners.set(binding.identifier.name, []);
        owners.get(binding.identifier.name).push(identity);
      }
    }
  }
  for (const [file] of ordered(sources)) {
    const record = analysis.files.get(file);
    for (const node of record.ast.program.body) if (external(node) && !loads.includes(node.source.value)) loads.push(node.source.value);
    references.push([file, record.lexical.filter(isLexicalReference).map(path => {
      const binding = path.scope.getBinding(path.node.name);
      return binding ? identities.get(binding) : owners.has(path.node.name) ? owners.get(path.node.name) : ['global', path.node.name];
    })]);
  }
  // Top-level references must use the same representation in owning and sibling files.
  const normalize = value => Array.isArray(value) && value.length === 1 && Array.isArray(value[0]) ? value[0] : value;
  return JSON.stringify({ bindings, references: references.map(([file, refs]) => [file, refs.map(normalize)]), loads });
}

export function checkImportMerge(before, after, options = {}) {
  try {
    if (JSON.stringify([...before.keys()]) !== JSON.stringify([...after.keys()])) return 'MERGE_FILES_CHANGED';
    const expected = rewrite(before, plan(before, options));
    // Exact replay permits only planned specifier removal and lexical reference edits.
    if ([...expected].some(([file, source]) => source !== after.get(file))) return 'MERGE_SOURCE_CHANGED';
    if (facts(before) !== facts(after)) return 'MERGE_BINDING_OR_LOAD_CHANGED';
    return null;
  } catch (error) { return `MERGE_CHECK_FAILED ${error.message}`; }
}
export function mergeExternalImports(sources, options = {}) {
  const after = rewrite(sources, plan(sources, options));
  if ([...sources].every(([file, source]) => after.get(file) === source)) return after;
  const reason = checkImportMerge(sources, after, options);
  if (reason) throw new Error(reason);
  return after;
}
export function runMergeFile(root, file, pool) {
  if (existsSync(join(root, '.unminify', 'rename-bindings', 'files', `${file}.jsonl`))) return { file, status: 'merge-imports-skipped' };
  const path = join(root, file), source = readFileSync(path, 'utf8');
  const journal = openJournal(root, file, source, { version: importMergeVersion }, 'restore-syntax/imports');
  if (journal.done) return { file, status: 'merge-imports-skipped' };
  const inputs = new Map([[file, source]]);
  function commit(output) {
    const after = output.get(file);
    journal.prepare(source, after, 'merge-imports');
    if (source !== after) atomicWrite(path, after);
    journal.complete(after, 'merge-imports');
    return { file, status: 'merge-imports', changed: source !== after };
  }
  return pool ? pool.run({ kind: 'merge', sources: inputs }).then(commit)
    : commit(mergeExternalImports(inputs));
}
