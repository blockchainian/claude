import { parse } from '@babel/parser';
import traverseModule from '@babel/traverse';
import { isLexicalIdentifier, isLexicalReference, isModelBinding, restoredImportEdit, identifierBinding } from './identifiers.mjs';
import { toIdentifier, bindingContext, applyRenames } from './rename.mjs';
import { checkRewrite } from './checks.mjs';
const traverse = traverseModule.default ?? traverseModule;

/** Extract cloneable facts, retaining no Babel objects beyond this file. */
export function analyzeImportChunkFile(source) {
  const ast = parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true, plugins: ['jsx'] });
  let program, hasWith = false, directEval = false;
  const paths = [], local = new Set(), exports = [];
  traverse(ast, {
    Program(path) { program = path; },
    Scope(path) { for (const binding of Object.values(path.scope.bindings)) local.add(binding); },
    WithStatement() { hasWith = true; },
    CallExpression(path) { if (path.get('callee').isIdentifier({ name: 'eval' }) && !path.scope.getBinding('eval')) directEval = true; },
    'Identifier|JSXIdentifier'(path) { if (isLexicalIdentifier(path)) paths.push(path); },
    ExportNamedDeclaration(path) { if (path.node.declaration) exports.push(path.node); },
  });
  const all = [...local].sort((a, b) => a.identifier.start - b.identifier.start);
  const top = Object.values(program.scope.bindings).sort((a, b) => a.identifier.start - b.identifier.start);
  const declarations = new Map(all.map(binding => [binding.identifier, binding]));
  const indices = new Map(all.map((binding, index) => [binding, index])), captures = new Map();
  const bindings = top.map((binding, index) => {
    const path = binding.path, namedImport = path.isImportSpecifier();
    return { index, name: binding.identifier.name, key: binding.identifier.start, end: binding.identifier.end,
      kind: binding.kind, model: isModelBinding(binding), namedImport,
      imported: namedImport ? path.node.imported.name ?? path.node.imported.value : null,
      source: binding.kind === 'module' ? path.parent.source.value : null,
      context: bindingContext(source, binding) };
  });
  const lexical = paths.map(path => {
    const binding = path.scope.getBinding(path.node.name), owner = identifierBinding(path, declarations);
    let value = path;
    if (value.parentPath.isAssignmentPattern() && value.key === 'left') value = value.parentPath;
    const shorthand = value.parentPath.isObjectProperty() && value.key === 'value' && value.parentPath.node.shorthand;
    const parent = path.parentPath;
    const restored = parent.isImportSpecifier() && path.key === 'local'
      ? restoredImportEdit(path, source, parent.node.imported.name) : null;
    const top = !binding || binding.scope === program.scope, reference = isLexicalReference(path);
    if (top && reference && !captures.has(path.scope)) captures.set(path.scope, Object.entries(path.scope.getAllBindings())
      .filter(([, other]) => other.scope !== program.scope).map(([name]) => name));
    const nested = top && reference ? captures.get(path.scope) : [];
    return { name: path.node.name, start: path.node.start, end: path.node.end,
      top, free: !binding,
      reference, jsx: path.isJSXIdentifier(), nested,
      identityLocal: owner && owner.scope !== program.scope ? indices.get(owner) : null,
      exported: parent.isExportSpecifier(), shorthand,
      importAlias: parent.isImportSpecifier() && parent.node.imported.start === path.node.start,
      exportAlias: parent.isExportSpecifier() && parent.node.exported.start === path.node.start,
      restored: restored ? { start: restored.start, end: restored.end, imported: parent.node.imported.name } : null };
  });
  return { bindings, lexical, hasWith, directEval,
    allBindings: all.map(binding => ({ name: binding.identifier.name, key: binding.identifier.start, top: binding.scope === program.scope })),
    exports: exports.map(node => ({ start: node.start,
      prefixEnd: node.start + /^export\s*/.exec(source.slice(node.start))[0].length,
      end: node.declaration.end, bindings: bindings.filter(binding => binding.key >= node.declaration.start && binding.end <= node.declaration.end).map(binding => binding.name) })) };
}

export function applyImportChunkFile(source, names, record) {
  const edits = new Map();
  const add = (start, end, text) => edits.set(`${start}:${end}`, { start, end, text });
  for (const token of record.lexical) {
    const name = names.get(token.name);
    if (!name || name === token.name || !token.top) continue;
    if (toIdentifier(name) !== name) throw new Error(`Invalid identifier: ${name}`);
    if (token.restored?.imported === name) { add(token.restored.start, token.restored.end, ''); continue; }
    const raw = source.slice(token.start, token.end);
    const text = token.shorthand ? `${raw}: ${name}` : token.importAlias ? `${raw} as ${name}` : token.exportAlias ? `${name} as ${raw}` : name;
    add(token.start, token.end, text);
  }
  for (const declaration of record.exports) {
    if (!declaration.bindings.some(name => names.has(name))) continue;
    add(declaration.start, declaration.prefixEnd, '');
    add(declaration.end, declaration.end, `\nexport { ${declaration.bindings.map(name => names.has(name) ? `${names.get(name)} as ${name}` : name).join(', ')} };`);
  }
  let cursor = 0, text = '';
  for (const edit of [...edits.values()].sort((a, b) => a.start - b.start || a.end - b.end)) {
    if (edit.start < cursor) throw new Error('Overlapping chunk edits');
    text += source.slice(cursor, edit.start) + edit.text; cursor = edit.end;
  }
  return text + source.slice(cursor);
}

export function checkImportChunkFile({ before, after, original, current, inverse }) {
  const oldBindings = original.allBindings.filter(binding => !binding.top), next = current.allBindings.filter(binding => !binding.top);
  if (oldBindings.length !== next.length) return 'CHUNK_BINDINGS_CHANGED';
  const names = new Map();
  for (let i = 0; i < oldBindings.length; i++) if (oldBindings[i].name !== next[i].name) names.set(next[i].key, oldBindings[i].name);
  let restored = names.size ? applyRenames(after, names) : after;
  const record = names.size ? analyzeImportChunkFile(restored) : current;
  restored = applyImportChunkFile(restored, new Map(inverse), record);
  return checkRewrite(before, restored, { fragment: false, sharedProgram: true });
}
