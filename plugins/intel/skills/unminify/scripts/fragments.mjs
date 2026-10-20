/** Detect files whose program bindings may be shared with sibling chunk slices. */
export function fragmentFacts(program) {
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
  // Unused bindings alone also occur in complete modules. Without exports they
  // can instead be declarations/imports consumed only by a sibling slice.
  const fragment = program.node.sourceType === 'script' || unboundExports.size > 0
    || (!exports.length && bindings.some(binding => !binding.referenced));
  return { fragment, unboundExports, topLevelNames: bindings
    .sort((a, b) => a.identifier.start - b.identifier.start).map(binding => binding.identifier.name) };
}
