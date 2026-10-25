import { timed } from './timing.mjs';
import { isLexicalIdentifier, isLexicalReference, restoredImportEdit, identifierBinding } from './identifiers.mjs';
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

export function sameImportedBinding(left, right) {
  return left.path.isImportSpecifier() && right.path.isImportSpecifier()
    && left.path.parent.source.value === right.path.parent.source.value
    && externalName(left.path.node.imported) === externalName(right.path.node.imported);
}

/** Prefer the external source when restoring an occupied named import. */
export function importCollisionName(binding, proposed) {
  const path = binding.path;
  if (!path.isImportSpecifier() || externalName(path.node.imported) !== proposed) return proposed;
  const source = path.parent.source.value;
  if (source.startsWith('.')) return proposed;
  const segment = source.replace(/^node:/, '').split('/').at(-1);
  const name = toIdentifier(`${segment.replace(/_+/g, '-')}-${proposed[0].toUpperCase()}${proposed.slice(1)}`);
  return name[0].toLowerCase() + name.slice(1);
}

/** Rank only competing external restores; leave unrelated proposals in place. */
export function orderImportRestores(proposals, records) {
  const groups = new Map();
  const external = binding => binding.path.isImportSpecifier()
    && !binding.path.parent.source.value.startsWith('.');
  for (const [key, raw] of proposals) {
    const record = records.get(key);
    if (!record || !external(record.binding) || externalName(record.binding.path.node.imported) !== raw) continue;
    if (!groups.has(raw)) groups.set(raw, []);
    groups.get(raw).push(key);
  }
  const replacements = new Map(), displaced = [];
  for (const [name, keys] of groups) {
    const occupants = [...records].filter(([, record]) => record.binding.identifier.name === name);
    if (occupants.some(([, record]) => !external(record.binding)
      || externalName(record.binding.path.node.imported) !== name)) continue;
    // An already restored import participates and wins ties in its current slot.
    const contenders = [...occupants.map(([key]) => key).filter(key => !keys.includes(key)), ...keys];
    if (new Set(contenders.map(key => records.get(key).binding.path.parent.source.value)).size < 2) continue;
    const winner = contenders.reduce((best, key) => records.get(key).references > records.get(best).references ? key : best);
    for (const [key, record] of occupants) {
      if (key !== winner && !sameImportedBinding(record.binding, records.get(winner).binding)) {
        displaced.push([key, importCollisionName(record.binding, name)]);
      }
    }
    const ordered = [winner, ...contenders.filter(key => key !== winner)]
      .filter(key => !displaced.some(([other]) => other === key));
    // Keep other proposal groups and non-import proposals in their original slots.
    keys.forEach((key, index) => replacements.set(key, index === 0 ? ordered.map(other => [other, name]) : []));
  }
  return [...displaced, ...[...proposals].flatMap(([key, raw]) => replacements.get(key) ?? [[key, raw]])];
}

function parseSource(source) {
  return timed('parse', () => parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true, plugins: ['jsx'] }));
}

function analyze(source, options) {
  const ast = parseSource(source);
  const bindings = new Map();
  const scopes = new Set();
  const globals = new Map();
  const jsxTags = new Set();
  const references = [];
  let program;
  traverse(ast, {
    Program(path) { program = path; },
    // Babel also gives parameter patterns scopes outside the Scopable alias.
    Scope(path) {
      for (let scope = path.scope; scope; scope = scope.parent) scopes.add(scope);
      for (const binding of Object.values(path.scope.bindings)) {
        const key = binding.identifier.start;
        if (!bindings.has(key)) bindings.set(key, { binding, scopes: new Set(), references: new Set(), name: binding.identifier.name });
        bindings.get(key).scopes.add(binding.scope);
      }
    },
    'Identifier|JSXIdentifier'(path) {
      if (!isLexicalIdentifier(path)) return;
      const binding = path.scope.getBinding(path.node.name);
      if (binding && isLexicalReference(path)) references.push([binding.identifier.start, path.scope]);
      if (binding) {
        if (path.isJSXIdentifier() && (path.parentPath.isJSXOpeningElement() || path.parentPath.isJSXClosingElement())) jsxTags.add(binding.identifier.start);
        return;
      }
      if (!globals.has(path.node.name)) globals.set(path.node.name, new Set());
      globals.get(path.node.name).add(path.scope);
    },
  });
  for (const [key, scope] of references) bindings.get(key).references.add(scope);
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
  return { ast, bindings, globals, contains, program, jsxTags, ...fragmentFacts(program, options) };
}

/** Give short declarations in full and large owners as declaration/reference excerpts. */
export function bindingContext(source, binding) {
  const path = binding.kind === 'param' ? binding.path.findParent(path => path.isFunction()) : binding.path;
  const node = path?.node ?? binding.path.node;
  const text = source.slice(node.start, node.end);
  if (text.length <= 2000) return `Binding ${binding.identifier.name}:\n${text}`;
  const excerpts = [text.slice(0, 900)];
  for (const reference of binding.referencePaths.slice(0, 3)) {
    const statement = reference.getStatementParent()?.node ?? reference.node;
    const start = Math.max(statement.start, reference.node.start - 150);
    excerpts.push(source.slice(start, Math.min(statement.end, start + 350)));
  }
  return `Binding ${binding.identifier.name}:\n${excerpts.join('\n...\n')}`;
}

export function resolveNames(source, proposals, options) {
  const { bindings, globals, contains, jsxTags, fragment } = analyze(source, options);
  const byName = new Map();
  for (const record of bindings.values()) {
    if (!byName.has(record.name)) byName.set(record.name, new Set());
    byName.get(record.name).add(record);
  }
  const result = new Map();
  const ordered = options?.restoreExternalImports
    ? orderImportRestores(proposals, new Map([...bindings].map(([key, record]) => [key, {
      binding: record.binding, references: record.binding.referencePaths.length,
    }]))) : proposals;
  for (const [key, raw] of ordered) {
    const record = bindings.get(key);
    if (!record) throw new Error(`Unknown binding key: ${key}`);
    if (fragment && record.binding.scope.path.isProgram()) continue;
    let proposed = toIdentifier(raw);
    if (jsxTags.has(key) && /^[a-z]/.test(proposed)) proposed = proposed[0].toUpperCase() + proposed.slice(1);
    if (proposed === record.name) continue;
    const collides = (candidate, differentImport = false) => {
      for (const other of byName.get(candidate) ?? []) {
        if (other === record || differentImport && sameImportedBinding(record.binding, other.binding)) continue;
        for (const scope of record.scopes) for (const otherScope of other.scopes) {
          if (scope === otherScope
            || contains(scope, otherScope) && [...record.references].some(reference => contains(otherScope, reference))
            || contains(otherScope, scope) && [...other.references].some(reference => contains(scope, reference))) return true;
        }
      }
      for (const scope of record.scopes) for (const globalScope of globals.get(candidate) ?? []) {
        if (contains(scope, globalScope)) return true;
      }
      return false;
    };
    if (options?.restoreExternalImports && collides(proposed, true)) proposed = importCollisionName(record.binding, proposed);
    let name = proposed;
    if (collides(name)) {
      options?.onCollision?.({ key, proposed, occupied: [...new Set([...byName.keys(), ...globals.keys()])].filter(candidate => collides(candidate)), context: bindingContext(source, record.binding) });
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
  const declarations = new Map([...bindings.values()].map(({ binding }) => [binding.identifier, binding]));
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
      const binding = identifierBinding(path, declarations);
      const name = binding && renames.get(binding.identifier.start);
      if (!name || name === path.node.name) return;
      const restored = restoredImportEdit(path, source, name);
      if (restored) {
        add(restored.start, restored.end, restored.text);
        return;
      }
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

export function summarizeImportFile(source, { file, paths, fragment = false, chunkBindings, minSymbolLength = 4 }) {
  const targetFile = (specifier) => {
    if (!specifier.startsWith('./') && !specifier.startsWith('../')) return null;
    const base = resolve(dirname(file), specifier);
    for (const candidate of [base, `${base}.js`, `${base}.mjs`, resolve(base, 'index.js'), resolve(base, 'index.mjs')]) {
      if (paths.has(candidate)) return paths.get(candidate);
    }
    return null;
  };

  const analysis = analyze(source, { fragment });
  const exports = new Map();
  const stars = [];
  const imports = [];
  const local = node => {
    const binding = analysis.program.scope.getBinding(node.name);
    return binding ? { file, key: binding.identifier.start } : chunkBindings?.get(node.name) ?? null;
  };
  for (const node of analysis.ast.program.body) {
    if (node.type === 'ImportDeclaration') {
      const target = targetFile(node.source.value);
      for (const specifier of node.specifiers) {
        if (specifier.type === 'ImportNamespaceSpecifier') continue;
        if (target === null && (specifier.type !== 'ImportSpecifier'
          || externalName(specifier.imported).length < minSymbolLength
          || specifier.local.name === externalName(specifier.imported))) continue;
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
          ? { target: targetFile(node.source.value), name: externalName(specifier.local) }
          : local(specifier.local));
      }
    } else if (node.type === 'ExportDefaultDeclaration') {
      const declaration = node.declaration;
      if (declaration.type === 'Identifier') exports.set('default', local(declaration));
      else if (declaration.id) exports.set('default', local(declaration.id));
    } else if (node.type === 'ExportAllDeclaration') {
      const target = targetFile(node.source.value);
      if (target !== null) stars.push(target);
    }
  }
  // Retain names and offsets across modules, never Babel nodes or scopes.
  const bindings = new Map(Object.values(analysis.program.scope.bindings)
    .map(binding => [binding.identifier.start, binding.identifier.name]));
  return { source, bindings, exports, stars, imports, fragment: analysis.fragment };
}

function importPlanner(modules, chunks) {
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
  function* plan(file, visiting = new Set()) {
    if (resolved.has(file)) return resolved.get(file);
    if (visiting.has(file)) return new Map();
    const members = chunks?.groupFiles?.(file) ?? [file];
    const branch = new Set([...visiting, ...members]), grouped = new Map();
    for (const member of members) {
      const module = modules.get(member), proposals = new Map();
      grouped.set(member, proposals);
      if (chunks?.readOnly?.has(member) || module.fragment && !chunks?.chunkBindings.has(member)) continue;
      for (const imported of module.imports) {
        if (imported.target === null) { proposals.set(imported.key, imported.name); continue; }
        const exported = findExport(imported.target, imported.name);
        if (!exported) continue;
        const names = yield* plan(exported.file, branch);
        const name = names.get(exported.key) ?? modules.get(exported.file).bindings.get(exported.key);
        if (chunks?.chunkBindings.has(member) && !names.has(exported.key)
          && !chunks.renamedExports.has(`${exported.file}\0${name}`)) continue;
        proposals.set(imported.key, name);
      }
    }
    if (chunks?.groupFiles?.(file)) {
      const names = yield { file, grouped };
      for (const member of members) resolved.set(member, names.get(member));
    } else {
      const proposals = grouped.get(file);
      if (chunks?.readOnly?.has(file) || modules.get(file).fragment && !chunks?.chunkBindings.has(file)) resolved.set(file, proposals);
      else resolved.set(file, yield { file, proposals });
    }
    return resolved.get(file);
  }
  return { plan, resolved };
}

export function followImports(files, withFile = (file, task) => task(), fragments = new Set(), chunks, { minSymbolLength = 4, pool } = {}) {
  if (pool) return followImportsParallel(files, withFile, fragments, chunks, { minSymbolLength, pool });
  const modules = new Map();
  const paths = new Map([...files.keys()].map(file => [resolve(file), file]));
  for (const [file, source] of files) {
    modules.set(file, withFile(file, () => summarizeImportFile(source, {
      file, paths, fragment: fragments.has(file), chunkBindings: chunks?.chunkBindings.get(file), minSymbolLength,
    })));
  }
  const { plan } = importPlanner(modules, chunks);
  const namesFor = file => {
    const iterator = plan(file);
    let step = iterator.next();
    while (!step.done) {
      const { file, proposals, grouped } = step.value;
      const names = grouped ? chunks.resolveGroup(grouped) : chunks?.chunkBindings.has(file) ? chunks.resolve(file, proposals)
        : withFile(file, () => timed('resolveNames', () => resolveNames(modules.get(file).source, proposals, {
          fragment: fragments.has(file), restoreExternalImports: true,
        })));
      step = iterator.next(names);
    }
    return step.value;
  };
  const changed = new Map();
  for (const [file, module] of modules) {
    const names = namesFor(file);
    if (names.size && !chunks?.chunkBindings.has(file)) changed.set(file, withFile(file, () => timed('applyRenames', () => applyRenames(module.source, names))));
  }
  return changed;
}

async function followImportsParallel(files, withFile, fragments, chunks, { minSymbolLength, pool }) {
  const modules = new Map();
  const paths = new Map([...files.keys()].map(file => [resolve(file), file]));
  const entries = [...files];
  const summaries = await pool.map(entries, ([file, source]) => withFile(file, () => pool.run({
    kind: 'import-analyze', source, options: {
      file, paths, fragment: fragments.has(file), chunkBindings: chunks?.chunkBindings.get(file), minSymbolLength,
    },
  })));
  entries.forEach(([file], index) => modules.set(file, summaries[index]));
  const { plan, resolved } = importPlanner(modules, chunks);
  // Resolve dependency chains in insertion order, preserving cyclic graph behavior.
  for (const file of modules.keys()) {
    const iterator = plan(file);
    let step = iterator.next();
    while (!step.done) {
      const { file, proposals, grouped } = step.value;
      const names = grouped ? await chunks.resolveGroup(grouped) : chunks?.chunkBindings.has(file) ? await chunks.resolve(file, proposals)
        : await withFile(file, () => pool.run({ kind: 'import-resolve', source: modules.get(file).source, proposals,
          options: { fragment: fragments.has(file), restoreExternalImports: true } }));
      step = iterator.next(names);
    }
  }
  const edits = await pool.map(entries, async ([file, source]) => {
    const names = resolved.get(file);
    if (!names.size || chunks?.chunkBindings.has(file)) return null;
    return withFile(file, () => pool.run({ kind: 'import-apply', source, names }));
  });
  return new Map(entries.flatMap(([file], index) => edits[index] === null ? [] : [[file, edits[index]]]));
}
