import { timed } from './timing.mjs';
import { isLexicalIdentifier } from './identifiers.mjs';
import { fragmentFacts } from './fragments.mjs';
import { parse } from '@babel/parser';
import traverseModule from '@babel/traverse';
import { dirname, resolve } from 'node:path';

const traverse = traverseModule.default ?? traverseModule;
const reserved = new Set(('break case catch class const continue debugger default delete do else export extends finally for function if import in instanceof new return super switch this throw try typeof var void while with yield enum implements interface let package private protected public static null true false await eval arguments').split(' '));
const startCharacter = /^[$_\p{ID_Start}]$/u;
const continueCharacter = /^[$_\u200c\u200d\p{ID_Continue}]$/u;
const identifier = /^[$_\p{ID_Start}][$_\u200c\u200d\p{ID_Continue}]*$/u;

export function toIdentifier(raw) {
  if (typeof raw !== 'string') throw new TypeError('A proposed name must be a string');
  let result = '';
  let capitalize = false;
  for (const character of raw) {
    if (/[.\s-]/u.test(character)) {
      if (result) capitalize = true;
    } else if (continueCharacter.test(character)) {
      result += capitalize ? character.toUpperCase() : character;
      capitalize = false;
    }
  }
  if (!result) return '_';
  if (!startCharacter.test([...result][0]) || reserved.has(result)) result = `_${result}`;
  return result;
}

function parseSource(source) {
  return timed('parse', () => parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true, plugins: ['jsx'] }));
}

function analyze(source) {
  const ast = parseSource(source);
  const bindings = new Map();
  const scopes = new Set();
  const globals = new Map();
  const jsxTags = new Set();
  let program;
  traverse(ast, {
    Program(path) { program = path; },
    Scopable(path) {
      for (let scope = path.scope; scope; scope = scope.parent) scopes.add(scope);
      for (const binding of Object.values(path.scope.bindings)) {
        const key = binding.identifier.start;
        if (!bindings.has(key)) bindings.set(key, { binding, scopes: new Set(), name: binding.identifier.name });
        bindings.get(key).scopes.add(binding.scope);
      }
    },
    'Identifier|JSXIdentifier'(path) {
      if (!isLexicalIdentifier(path)) return;
      const binding = path.scope.getBinding(path.node.name);
      if (binding) {
        if (path.isJSXIdentifier() && (path.parentPath.isJSXOpeningElement() || path.parentPath.isJSXClosingElement())) jsxTags.add(binding.identifier.start);
        return;
      }
      if (!globals.has(path.node.name)) globals.set(path.node.name, new Set());
      globals.get(path.node.name).add(path.scope);
    },
  });
  // Scope intervals make same/ancestor/descendant checks independent of depth.
  const children = new Map([...scopes].map(scope => [scope, []]));
  for (const scope of scopes) if (scope.parent) children.get(scope.parent).push(scope);
  const intervals = new Map();
  let clock = 0;
  const stack = [[program.scope, false]];
  while (stack.length) {
    const [scope, leaving] = stack.pop();
    if (leaving) intervals.get(scope).end = clock;
    else {
      intervals.set(scope, { start: clock++, end: null });
      stack.push([scope, true]);
      for (const child of children.get(scope)) stack.push([child, false]);
    }
  }
  const contains = (outer, inner) => {
    const a = intervals.get(outer);
    const b = intervals.get(inner);
    return a.start <= b.start && b.start < a.end;
  };
  return { ast, bindings, globals, contains, program, jsxTags, ...fragmentFacts(program) };
}

export function resolveNames(source, proposals) {
  const { bindings, globals, contains, jsxTags, fragment } = analyze(source);
  const byName = new Map();
  for (const record of bindings.values()) {
    if (!byName.has(record.name)) byName.set(record.name, new Set());
    byName.get(record.name).add(record);
  }
  const result = new Map();
  for (const [key, raw] of proposals) {
    const record = bindings.get(key);
    if (!record) throw new Error(`Unknown binding key: ${key}`);
    if (fragment && record.binding.scope.path.isProgram()) continue;
    let proposed = toIdentifier(raw);
    if (jsxTags.has(key) && /^[a-z]/.test(proposed)) proposed = proposed[0].toUpperCase() + proposed.slice(1);
    if (proposed === record.name) continue;
    const collides = candidate => {
      for (const other of byName.get(candidate) ?? []) {
        if (other === record) continue;
        for (const scope of record.scopes) for (const otherScope of other.scopes) {
          if (contains(scope, otherScope) || contains(otherScope, scope)) return true;
        }
      }
      for (const scope of record.scopes) for (const globalScope of globals.get(candidate) ?? []) {
        if (contains(scope, globalScope)) return true;
      }
      return false;
    };
    let name = proposed;
    if (collides(name)) {
      const suffix = /^(.*?)(\d+)$/.exec(proposed);
      const stem = suffix ? suffix[1] : proposed;
      let number = suffix ? BigInt(suffix[2]) + 1n : 2n;
      do { name = `${stem}${number++}`; } while (collides(name));
    }
    byName.get(record.name).delete(record);
    if (!byName.has(name)) byName.set(name, new Set());
    byName.get(name).add(record);
    record.name = name;
    if (name !== record.binding.identifier.name) result.set(key, name);
  }
  return result;
}

function shorthandProperty(path) {
  let value = path;
  if (value.parentPath.isAssignmentPattern() && value.key === 'left') value = value.parentPath;
  return value.parentPath.isObjectProperty() && value.key === 'value' && value.parentPath.node.shorthand
    ? value.parentPath.node : null;
}

export function applyRenames(source, renames) {
  if (!renames.size) return source;
  const { ast, bindings, jsxTags } = analyze(source);
  for (const [key, name] of renames) {
    if (!bindings.has(key)) throw new Error(`Unknown binding key: ${key}`);
    if (jsxTags.has(key) && /^[a-z]/.test(name)) throw new Error(`JSX tag binding must retain a lexical tag name: ${name}`);
    if (typeof name !== 'string' || !identifier.test(name) || reserved.has(name)) throw new Error(`Invalid identifier: ${name}`);
  }
  const edits = new Map();
  const add = (start, end, text) => {
    const key = `${start}:${end}`;
    const previous = edits.get(key);
    if (previous && previous.text !== text) throw new Error(`Conflicting edits at ${start}`);
    edits.set(key, { start, end, text });
  };
  traverse(ast, {
    'Identifier|JSXIdentifier'(path) {
      if (!isLexicalIdentifier(path)) return;
      const parent = path.parentPath;
      const binding = path.scope.getBinding(path.node.name);
      const name = binding && renames.get(binding.identifier.start);
      if (!name || name === path.node.name) return;
      const { start, end } = path.node;
      const raw = source.slice(start, end);
      let replacement = name;
      if (shorthandProperty(path)) replacement = `${raw}: ${name}`;
      else if (parent.isImportSpecifier() && parent.node.imported.start === start && parent.node.imported.end === end) replacement = `${raw} as ${name}`;
      else if (parent.isExportSpecifier() && parent.node.exported.start === start && parent.node.exported.end === end) replacement = `${name} as ${raw}`;
      add(start, end, replacement);
    },
    ExportNamedDeclaration(path) {
      const declaration = path.node.declaration;
      if (!declaration) return;
      const exported = [...bindings.values()].filter(({ binding }) => binding.scope === path.scope && binding.identifier.start >= declaration.start && binding.identifier.end <= declaration.end);
      if (!exported.some(({ binding }) => renames.has(binding.identifier.start))) return;
      // Keep the declaration text and its comments; move only the export syntax.
      const prefix = /^export\s*/.exec(source.slice(path.node.start))[0];
      add(path.node.start, path.node.start + prefix.length, '');
      const specifiers = exported.map(({ binding }) => {
        const original = binding.identifier.name;
        const name = renames.get(binding.identifier.start) ?? original;
        return name === original ? original : `${name} as ${original}`;
      });
      add(declaration.end, declaration.end, `\nexport { ${specifiers.join(', ')} };`);
    },
  });
  const sorted = [...edits.values()].sort((a, b) => a.start - b.start || a.end - b.end);
  let output = '';
  let cursor = 0;
  for (const edit of sorted) {
    if (edit.start < cursor) throw new Error(`Overlapping edits at ${edit.start}`);
    output += source.slice(cursor, edit.start) + edit.text;
    cursor = edit.end;
  }
  return output + source.slice(cursor);
}

function externalName(node) {
  return node.name ?? node.value;
}

export function followImports(files, withFile = (file, task) => task()) {
  const modules = new Map();
  const paths = new Map([...files.keys()].map(file => [resolve(file), file]));
  const targetFile = (file, specifier) => {
    if (!specifier.startsWith('./') && !specifier.startsWith('../')) return null;
    const base = resolve(dirname(file), specifier);
    for (const candidate of [base, `${base}.js`, `${base}.mjs`, resolve(base, 'index.js'), resolve(base, 'index.mjs')]) {
      if (paths.has(candidate)) return paths.get(candidate);
    }
    return null;
  };
  for (const [file, source] of files) {
    modules.set(file, withFile(file, () => {
      const analysis = analyze(source);
      const exports = new Map();
      const stars = [];
      const imports = [];
      const local = node => {
        const binding = analysis.program.scope.getBinding(node.name);
        return binding ? { file, key: binding.identifier.start } : null;
      };
      for (const node of analysis.ast.program.body) {
        if (node.type === 'ImportDeclaration') {
          const target = targetFile(file, node.source.value);
          if (target === null) continue;
          for (const specifier of node.specifiers) {
            if (specifier.type === 'ImportNamespaceSpecifier') continue;
            imports.push({ key: specifier.local.start, target, name: specifier.type === 'ImportDefaultSpecifier' ? 'default' : externalName(specifier.imported) });
          }
        } else if (node.type === 'ExportNamedDeclaration') {
          if (node.declaration) {
            for (const { binding } of analysis.bindings.values()) {
              if (binding.scope === analysis.program.scope && binding.identifier.start >= node.declaration.start && binding.identifier.end <= node.declaration.end) exports.set(binding.identifier.name, { file, key: binding.identifier.start });
            }
          }
          for (const specifier of node.specifiers) {
            if (specifier.type !== 'ExportSpecifier') continue;
            exports.set(externalName(specifier.exported), node.source
              ? { target: targetFile(file, node.source.value), name: externalName(specifier.local) }
              : local(specifier.local));
          }
        } else if (node.type === 'ExportDefaultDeclaration') {
          const declaration = node.declaration;
          if (declaration.type === 'Identifier') exports.set('default', local(declaration));
          else if (declaration.id) exports.set('default', local(declaration.id));
        } else if (node.type === 'ExportAllDeclaration') {
          const target = targetFile(file, node.source.value);
          if (target !== null) stars.push(target);
        }
      }
      // Retain names and offsets across modules, never Babel nodes or scopes.
      const bindings = new Map(Object.values(analysis.program.scope.bindings)
        .map(binding => [binding.identifier.start, binding.identifier.name]));
      return { source, bindings, exports, stars, imports, fragment: analysis.fragment };
    }));
  }
  function findExport(file, name, seen = new Set()) {
    const token = JSON.stringify([file, name]);
    if (seen.has(token)) return null;
    const branch = new Set(seen).add(token);
    const module = modules.get(file);
    const exported = module.exports.get(name);
    if (module.exports.has(name)) {
      if (!exported) return null;
      if ('key' in exported) return exported;
      return exported.target === null ? null : findExport(exported.target, exported.name, branch);
    }
    if (name === 'default') return null;
    let found = null;
    for (const target of module.stars) {
      const candidate = findExport(target, name, branch);
      if (!candidate) continue;
      if (found && (found.file !== candidate.file || found.key !== candidate.key)) return null;
      found = candidate;
    }
    return found;
  }
  const resolved = new Map();
  function plan(file, visiting = new Set()) {
    if (resolved.has(file)) return resolved.get(file);
    if (visiting.has(file)) return new Map();
    const branch = new Set(visiting).add(file);
    const module = modules.get(file);
    const proposals = new Map();
    if (module.fragment) {
      resolved.set(file, proposals);
      return proposals;
    }
    for (const imported of module.imports) {
      const exported = findExport(imported.target, imported.name);
      if (!exported) continue;
      const names = plan(exported.file, branch);
      const name = names.get(exported.key) ?? modules.get(exported.file).bindings.get(exported.key);
      proposals.set(imported.key, name);
    }
    const names = withFile(file, () => timed('resolveNames', () => resolveNames(module.source, proposals)));
    resolved.set(file, names);
    return names;
  }
  const changed = new Map();
  for (const [file, module] of modules) {
    const names = plan(file);
    if (names.size) changed.set(file, withFile(file, () => timed('applyRenames', () => applyRenames(module.source, names))));
  }
  return changed;
}
