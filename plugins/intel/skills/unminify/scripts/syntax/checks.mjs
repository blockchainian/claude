// Ported from wakaru v1.14.0 (Apache-2.0); source-range safety checks.
import { parseSyntax, traverse } from './guards.mjs';
import { isLexicalIdentifier } from '../identifiers.mjs';
import { fragmentFacts } from '../fragments.mjs';

// Grouping and sequence wrappers have no evaluation event of their own. Keep
// declaration commits, reads, calls and control regions in their execution order.
// Function bodies belong to complete local scopes, not Program initialization.
function evaluationOrder(program) {
  const events = [];
  const intrinsicValues = new Set(['undefined', 'NaN', 'Infinity']);
  const control = path => path.isStatement() && !path.isBlockStatement()
    && !path.isExpressionStatement() && !path.isVariableDeclaration()
    && !path.isFunctionDeclaration() && !path.isEmptyStatement()
    || path.isConditionalExpression() || path.isLogicalExpression();
  const declaration = path => {
    const names = Object.keys(path.getOuterBindingIdentifiers());
    return names.filter(name => path.scope.getBinding(name)?.scope.path.isProgram());
  };
  // These fields run once before entering the statement's controlled region.
  const eager = { ForStatement: 'init', IfStatement: 'test', SwitchStatement: 'discriminant', WithStatement: 'object' };
  const started = new WeakSet();
  const region = path => control(path.parentPath) && eager[path.parent.type] !== path.key
    ? path.listKey ?? path.key : null;
  function constant(path) {
    const { confident, value } = path.evaluate();
    if (!confident || (value !== null && !['undefined', 'boolean', 'number', 'string', 'bigint'].includes(typeof value))) return null;
    return [typeof value, Object.is(value, -0) ? '-0' : String(value)];
  }
  function enter(path) {
    if (eager[path.parent.type] && eager[path.parent.type] !== path.key && !started.has(path.parent)) {
      events.push(['enter', path.parent.type, null]);
      started.add(path.parent);
    }
    if (region(path) !== null) events.push(['region', region(path)]);
    if (path.isFunction()) {
      if (path.node.computed) {
        // Computed method keys run while the surrounding object/class is built.
        const key = path.get('key');
        enter(key);
        if (key.isIdentifier()) identifier(key);
        key.traverse(visitor);
        exit(key);
      }
      if (path.isFunctionDeclaration() && declaration(path).length) {
        events.push(['function', declaration(path)]);
      }
      path.skip();
      return;
    }
    if (control(path) && !eager[path.node.type]) events.push(['enter', path.node.type, path.node.operator ?? null]);
  }
  function exit(path) {
    if (path.isVariableDeclarator()) {
      events.push(['declare', path.parent.kind, path.node.id.type,
        Object.keys(path.getOuterBindingIdentifiers()), path.node.init !== null]);
    }
    if (path.isMemberExpression() || path.isOptionalMemberExpression()) {
      const property = path.node.property;
      const name = !path.node.computed && property.type === 'Identifier' ? property.name
        : property.type === 'StringLiteral' ? property.value : null;
      if (name !== null) events.push(['property', name]);
    }
    if (path.isCallExpression() || path.isOptionalCallExpression() || path.isNewExpression()) {
      events.push([path.node.type, path.get('arguments').map(constant)]);
    }
    if (path.isAssignmentExpression()) events.push([path.node.type, path.node.operator, constant(path.get('right'))]);
    if (path.isUpdateExpression()) events.push([path.node.type, path.node.operator, path.node.prefix]);
    if (path.isAwaitExpression() || path.isYieldExpression()) events.push([path.node.type]);
    if (control(path)) events.push(['exit', path.node.type]);
    if (region(path) !== null) events.push(['endRegion', region(path)]);
  }
  function identifier(path) {
    if (isLexicalIdentifier(path) && path.isReferencedIdentifier()
      && !intrinsicValues.has(path.node.name)) events.push(['read', path.node.name]);
  }
  const visitor = { enter, exit, 'Identifier|JSXIdentifier': identifier };
  program.traverse(visitor);
  return events;
}
function facts(ast, options) {
  let fragment, bindings, layout;
  const globals = new Set();
  traverse(ast, {
    Program(path) {
      ({fragment} = fragmentFacts(path, options));
      bindings = Object.values(path.scope.bindings).map(b => [b.identifier.name,b.kind]).sort();
      layout = fragment ? evaluationOrder(path) : [];
    },
    'Identifier|JSXIdentifier'(path) {
      if (isLexicalIdentifier(path) && !path.scope.getBinding(path.node.name)) globals.add(path.node.name);
    },
  });
  return {fragment,bindings:JSON.stringify(bindings),layout:JSON.stringify(layout),globals};
}
export function checkSyntaxRewrite(before, after, options = {}) {
  let a,b;
  try { a=facts(parseSyntax(before),options); b=facts(parseSyntax(after),options); }
  catch { return 'PARSE_FAIL'; }
  if (a.fragment && a.bindings!==b.bindings) return 'FRAGMENT_BINDINGS_CHANGED';
  if (a.fragment && a.layout!==b.layout) return 'FRAGMENT_DECLARATIONS_CHANGED';
  const allowed = new Set((options.allowedGlobals ?? []).filter(name => !a.fragment
    || (options.fragmentContext !== undefined && !options.fragmentContext.topLevelBindings.has(name))));
  const added = [...b.globals].filter(name=>!a.globals.has(name) && !allowed.has(name));
  return added.length ? `NEW_GLOBALS ${added.join(',')}` : null;
}
