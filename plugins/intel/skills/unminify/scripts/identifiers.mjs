// Lexical names exclude labels and public property/import/export names.
export function isLexicalIdentifier(path) {
  if (path.isJSXIdentifier()) {
    const parent = path.parentPath;
    if (parent.isJSXMemberExpression()) return path.key === 'object';
    return (parent.isJSXOpeningElement() || parent.isJSXClosingElement()) && path.key === 'name' && !/^[a-z]/.test(path.node.name);
  }
  const parent = path.parentPath;
  // Babel's isBindingIdentifier includes label declarations in its binding keys.
  if (path.key === 'label' && (parent.isLabeledStatement() || parent.isBreakStatement() || parent.isContinueStatement())) return false;
  if (parent.isImportSpecifier() && path.key === 'imported') return false;
  if (parent.isExportSpecifier() && (path.key === 'exported' || parent.parentPath.node.source)) return false;
  if (parent.isObjectProperty() && path.key === 'key' && !parent.node.computed) return false;
  return path.isReferencedIdentifier() || path.isBindingIdentifier();
}

/** Reads and assignment targets count as uses; declaration sites do not. */
export function isLexicalReference(path) {
  if (!isLexicalIdentifier(path)) return false;
  if (path.isJSXIdentifier() || path.isReferencedIdentifier()) return true;
  let target = path;
  while (target.parentPath && (target.parentPath.isObjectProperty()
    || target.parentPath.isObjectPattern() || target.parentPath.isArrayPattern()
    || target.parentPath.isRestElement() || target.parentPath.isAssignmentPattern())) target = target.parentPath;
  const parent = target.parentPath;
  return target.key === 'left' && (parent?.isAssignmentExpression() || parent?.isForInStatement() || parent?.isForOfStatement());
}

/** Named imports follow exports; default and namespace locals can use model names. */
export function isModelBinding(binding) {
  return binding.kind !== 'module' || !binding.path.isImportSpecifier();
}

/** Collapse a restored package alias with the canonical alias spacing. */
export function restoredImportEdit(path, source, name) {
  const specifier = path.parentPath;
  if (!specifier.isImportSpecifier() || path.key !== 'local') return null;
  const imported = specifier.node.imported;
  const module = specifier.parentPath.node.source.value;
  if (imported.type !== 'Identifier' || imported.name !== name || module.startsWith('./') || module.startsWith('../')) return null;
  if (source.slice(imported.end, path.node.start) !== ' as ') return null;
  return { start: imported.end, end: path.node.end, text: '' };
}

/** Declaration nodes use their owning binding; uses resolve in their lexical scope. */
export function identifierBinding(path, declarations) {
  return declarations.has(path.node) ? declarations.get(path.node) : path.scope.getBinding(path.node.name);
}
