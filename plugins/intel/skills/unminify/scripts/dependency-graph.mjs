import { readFileSync, statSync } from 'node:fs';
import { resolve, relative, posix } from 'node:path';
import { parse } from '@babel/parser';
import traverseModule from '@babel/traverse';

const traverse = traverseModule.default ?? traverseModule;
const normalize = value => value.replaceAll('\\', '/');

/** Index source snapshots once; only selected files participate in dependency edges. */
export function buildDependencyGraph(files, { root = process.cwd(), sources } = {}) {
  const selected = [...new Set(files.map(file => normalize(relative(root, resolve(root, file)))))].sort();
  const selectedSet = new Set(selected);
  const modules = new Map();
  const nodes = [];
  const dependencies = new Map();
  function isFile(file) {
    if (selectedSet.has(file)) return true;
    try { return statSync(resolve(root, file)).isFile(); }
    catch (error) { if (error.code === 'ENOENT' || error.code === 'ENOTDIR') return false; throw error; }
  }
  function importTarget(file, source) {
    const target = posix.normalize(posix.join(posix.dirname(file), source));
    for (const candidate of [target, `${target}.js`, `${target}.mjs`, `${target}/index.js`, `${target}/index.mjs`]) {
      if (isFile(candidate)) return candidate;
    }
    // An unresolved relative specifier is still a location that must follow its importer.
    return target;
  }
  for (const id of selected) {
    let source;
    if (sources) {
      source = sources.get(id) ?? sources.get(resolve(root, id));
      if (typeof source !== 'string') throw new Error(`missing source snapshot: ${id}`);
    } else source = readFileSync(resolve(root, id), 'utf8');
    const ast = parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true, plugins: ['jsx'] });
    const node = { id, moduleId: null, imports: [] };
    const required = new Set();
    const wrappers = [];
    traverse(ast, {
      CallExpression(path) {
        if (!path.get('callee').isIdentifier({ name: 'webpackModule' }) || path.scope.getBinding('webpackModule')) return;
        const [module, factory] = path.get('arguments');
        if (!module?.isNumericLiteral() || !(factory?.isFunctionExpression() || factory?.isArrowFunctionExpression())) return;
        const requireParameter = factory.get('params')[2];
        if (!requireParameter?.isIdentifier()) return;
        if (node.moduleId !== null) throw new Error(`multiple webpack modules in ${id}`);
        node.moduleId = module.node.value;
        if (modules.has(node.moduleId)) throw new Error(`duplicate webpack module ID ${node.moduleId}: ${modules.get(node.moduleId)}, ${id}`);
        modules.set(node.moduleId, id);
        wrappers.push(factory.scope.getBinding(requireParameter.node.name));
      },
      'ImportDeclaration|ExportNamedDeclaration|ExportAllDeclaration'(path) {
        const literal = path.node.source;
        if (!literal || !literal.value.startsWith('.')) return;
        node.imports.push({ start: literal.start, end: literal.end, target: importTarget(id, literal.value), source: literal.value });
      },
    });
    for (const binding of wrappers) {
      for (const reference of binding.referencePaths) {
        const call = reference.parentPath;
        if (!call.isCallExpression() || call.node.callee !== reference.node) continue;
        const argument = call.node.arguments[0];
        if (call.node.arguments.length === 1 && argument?.type === 'NumericLiteral') required.add(argument.value);
      }
    }
    nodes.push(node);
    dependencies.set(id, required);
  }
  const edges = [];
  for (const node of nodes) {
    const targets = new Set(node.imports.filter(link => selectedSet.has(link.target)).map(link => link.target));
    for (const module of dependencies.get(node.id)) if (modules.has(module)) targets.add(modules.get(module));
    for (const target of [...targets].sort()) edges.push({ source: node.id, target });
  }
  return { nodes, edges };
}
