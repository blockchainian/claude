// Rejects rewrites that change module contracts, scope resolution or source text.
import { execFileSync } from 'node:child_process';
import { parse } from '@babel/parser';
import traverseModule from '@babel/traverse';

const traverse = traverseModule.default ?? traverseModule;
const allowedGlobals = new Set(['undefined', 'Infinity', 'NaN']);
const metadata = new Set(['start', 'end', 'loc', 'extra', 'leadingComments', 'innerComments', 'trailingComments', 'comments', 'tokens', 'shorthand']);

function externalName(node) {
  return node.name ?? node.value;
}

function moduleFacts(ast) {
  const names = new Set();
  const loads = [];
  function exportedBindings(node) {
    if (!node) return;
    if (node.type === 'Identifier') names.add(`export:${node.name}`);
    else if (node.type === 'ObjectPattern') {
      for (const property of node.properties) exportedBindings(property.type === 'RestElement' ? property.argument : property.value);
    } else if (node.type === 'ArrayPattern') {
      for (const element of node.elements) exportedBindings(element);
    } else if (node.type === 'AssignmentPattern') exportedBindings(node.left);
    else if (node.type === 'RestElement') exportedBindings(node.argument);
  }
  for (const node of ast.program.body) {
    if (node.source) {
      names.add(`module:${node.source.value}`);
      loads.push(node.source.value);
    }
    if (node.type === 'ImportDeclaration') {
      for (const specifier of node.specifiers) {
        names.add(`import:${node.source.value}:${specifier.imported ? externalName(specifier.imported) : specifier.type}`);
      }
    } else if (node.type === 'ExportNamedDeclaration') {
      for (const specifier of node.specifiers) {
        names.add(`export:${externalName(specifier.exported)}`);
        if (node.source && specifier.local) names.add(`reexport:${node.source.value}:${externalName(specifier.local)}:${externalName(specifier.exported)}`);
      }
      const declaration = node.declaration;
      if (declaration?.id) exportedBindings(declaration.id);
      for (const entry of declaration?.declarations ?? []) exportedBindings(entry.id);
    } else if (node.type === 'ExportDefaultDeclaration') names.add('export:default');
    else if (node.type === 'ExportAllDeclaration') names.add(`export:*:${node.source.value}`);
  }
  return { names: JSON.stringify([...names].sort()), loads: JSON.stringify(loads) };
}

function variableIdentifier(path) {
  const parent = path.parent;
  if ((parent.type === 'ObjectProperty' || parent.type === 'ObjectMethod' || parent.type === 'ClassMethod' || parent.type === 'ClassProperty') && path.key === 'key' && !parent.computed) return false;
  if ((parent.type === 'MemberExpression' || parent.type === 'OptionalMemberExpression') && path.key === 'property' && !parent.computed) return false;
  if (parent.type === 'ImportSpecifier' && path.key === 'imported') return false;
  if (parent.type === 'ExportSpecifier' && (path.key === 'exported' || path.parentPath.parent.source)) return false;
  return path.isReferencedIdentifier() || path.isBindingIdentifier();
}

function splitExportDeclarations(source, ast, result) {
  const edits = [];
  const exported = new Map();
  traverse(ast, {
    ExportNamedDeclaration(path) {
      if (path.node.declaration) {
        exported.set(path.node, Object.values(path.getBindingIdentifiers()).sort((a, b) => a.start - b.start));
      }
    },
  });
  let index = 0;
  for (const node of ast.program.body) {
    const declaration = node.type === 'ExportNamedDeclaration' && node.declaration;
    const next = result.program.body[index + 1];
    if (declaration && result.program.body[index]?.type === declaration.type &&
        next?.type === 'ExportNamedDeclaration' && !next.declaration && !next.source) {
      // Reproduce only rename's export syntax edits. The existing comparisons
      // still check every declaration byte and every exported binding target.
      const identifiers = exported.get(node);
      const prefix = /^export\s*/.exec(source.slice(node.start))[0];
      edits.push({ start: node.start, end: node.start + prefix.length, text: '' });
      edits.push({ start: declaration.end, end: declaration.end, text: `\nexport { ${identifiers.map(id => id.name).join(', ')} };` });
      index++;
    }
    index++;
  }
  if (!edits.length) return { source, ast };
  let text = '';
  let offset = 0;
  for (const edit of edits) {
    text += source.slice(offset, edit.start) + edit.text;
    offset = edit.end;
  }
  text += source.slice(offset);
  return { source: text, ast: parse(text, { sourceType: 'module' }) };
}

function header(source) {
  // Only line comments and blank lines between them belong to the header.
  return /^(?:[\t ]*\/\/[^\r\n]*(?:\r\n|\r|\n|$))(?:(?:[\t ]*(?:\r\n|\r|\n))*[\t ]*\/\/[^\r\n]*(?:\r\n|\r|\n|$))*/.exec(source)?.[0] ?? '';
}

function analyze(source, ast) {
  const bindings = new Set();
  const variables = new Map();
  const globals = new Set();
  let constReassigned = 0;
  let earlyUse = 0;
  traverse(ast, {
    Scope(path) {
      for (const binding of Object.values(path.scope.bindings)) bindings.add(binding);
    },
    Identifier(path) {
      if (!variableIdentifier(path)) return;
      const binding = path.scope.getBinding(path.node.name);
      variables.set(path.node, binding ?? null);
      if (!binding) globals.add(path.node.name);
    },
  });
  const ordered = [...bindings].sort((a, b) => a.identifier.start - b.identifier.start);
  const indices = new Map(ordered.map((binding, index) => [binding, index]));
  for (const binding of ordered) {
    if (binding.kind === 'const') constReassigned += binding.constantViolations.length;
    if (!['let', 'const'].includes(binding.kind) && binding.path.node.type !== 'ClassDeclaration') continue;
    for (const ref of binding.referencePaths) {
      // Babel includes an ExportNamedDeclaration in referencePaths for `export const X`.
      // Only actual identifier reads in the declaration's function can execute early.
      if (ref.isIdentifier() && ref.node.start < binding.identifier.start && ref.getFunctionParent() === binding.path.getFunctionParent()) earlyUse++;
    }
  }

  const references = [];
  function shape(value) {
    if (Array.isArray(value)) return value.map(shape);
    if (!value || typeof value !== 'object') return value;
    if (variables.has(value)) {
      const binding = variables.get(value);
      references.push(binding ? `binding:${indices.get(binding)}` : `global:${value.name}`);
      return { type: 'Identifier', name: '$' };
    }
    const result = {};
    for (const key of Object.keys(value).sort()) {
      if (!metadata.has(key)) result[key] = shape(value[key]);
    }
    return result;
  }
  const structure = JSON.stringify(shape(ast.program));

  // Babel already represents `{ e }` and `{ e: value }`, and implicit/explicit
  // import/export aliases, with the same tree (apart from shorthand metadata).
  // Canonicalize just their spelling too. Keep property/export/import names fixed;
  // declaration/reference names become markers. No other bytes are discarded.
  const edits = [];
  const covered = new Set();
  traverse(ast, {
    ObjectProperty(path) {
      const node = path.node;
      const value = node.value.type === 'AssignmentPattern' ? node.value.left : node.value;
      if (node.shorthand && node.key.type === 'Identifier' && value.type === 'Identifier') {
        edits.push({ start: node.start, end: value.start, text: `${source.slice(node.key.start, node.key.end)}: ` });
      }
    },
    ImportSpecifier(path) {
      const node = path.node;
      if (node.imported.start === node.local.start) {
        edits.push({ start: node.start, end: node.end, text: `${source.slice(node.imported.start, node.imported.end)} as $` });
        covered.add(node.local);
      }
    },
    ExportSpecifier(path) {
      if (path.parent.source) return;
      const node = path.node;
      if (node.local.start === node.exported.start) {
        edits.push({ start: node.start, end: node.end, text: `$ as ${source.slice(node.exported.start, node.exported.end)}` });
        covered.add(node.local);
      }
    },
  });
  for (const node of variables.keys()) {
    if (!covered.has(node)) {
      edits.push({ start: node.start, end: node.end, text: '$' });
    }
  }
  edits.sort((a, b) => a.start - b.start || a.end - b.end);
  let text = '';
  let offset = 0;
  for (const edit of edits) {
    if (edit.start < offset) throw new Error('Overlapping normalization edits');
    text += source.slice(offset, edit.start) + edit.text;
    offset = edit.end;
  }
  text += source.slice(offset);
  return { globals, constReassigned, earlyUse, structure, references, text };
}

/** Returns null on success, otherwise a single-line rejection reason. */
export function checkRewrite(before, after) {
  let original;
  let result;
  try {
    execFileSync(process.execPath, ['--input-type=module', '--check'], { input: after, stdio: ['pipe', 'pipe', 'pipe'], timeout: 10000 });
    original = parse(before, { sourceType: 'module' });
    result = parse(after, { sourceType: 'module' });
  } catch {
    return 'PARSE_FAIL';
  }
  const originalModule = moduleFacts(original);
  const resultModule = moduleFacts(result);
  if (originalModule.names !== resultModule.names) return 'INTERFACE_CHANGED';
  if (originalModule.loads !== resultModule.loads) return 'LOAD_ORDER_CHANGED';
  let a;
  let b;
  try {
    const normalized = splitExportDeclarations(before, original, result);
    a = analyze(normalized.source, normalized.ast);
    b = analyze(after, result);
  } catch (error) {
    return `SCOPE_ANALYSIS_FAILED ${String(error.message).split(/[\r\n]/)[0]}`;
  }
  const globals = [...b.globals].filter(name => !a.globals.has(name) && !allowedGlobals.has(name));
  if (globals.length) return `NEW_GLOBALS ${globals.join(',')}`;
  if (b.constReassigned > a.constReassigned) return 'CONST_REASSIGNED';
  if (b.earlyUse > a.earlyUse) return 'EARLY_USE';
  if (header(before) !== header(after)) return 'HEADER_CHANGED';
  if (a.structure !== b.structure || a.text !== b.text) return 'STRUCTURE_OR_TEXT_CHANGED';
  if (JSON.stringify(a.references) !== JSON.stringify(b.references)) return 'BINDING_CHANGED';
  return null;
}
