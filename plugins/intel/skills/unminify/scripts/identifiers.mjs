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
