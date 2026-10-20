// Ported from wakaru v1.14.0 (Apache-2.0); source-range safety checks.
import { parseSyntax, traverse } from './guards.mjs';
import { isLexicalIdentifier } from '../identifiers.mjs';
import { fragmentFacts } from '../fragments.mjs';
function facts(ast, options) {
  let fragment, bindings, layout;
  const globals = new Set();
  traverse(ast, {
    Program(path) {
      ({fragment} = fragmentFacts(path, options));
      bindings = Object.values(path.scope.bindings).map(b => [b.identifier.name,b.kind]).sort();
      layout = [];
    },
    enter(path) {
      if (path.isVariableDeclaration() || path.isFunctionDeclaration() || path.isClassDeclaration() || path.isImportDeclaration()) {
        const ids = path.getOuterBindingIdentifiers();
        const programBindings = Object.keys(ids).filter(name => path.scope.getBinding(name)?.scope.path.isProgram());
        if (!programBindings.length) return;
        const position = [];
        for (let p=path; p.parentPath; p=p.parentPath) position.unshift([p.listKey ?? null,p.key]);
        const groups = path.isVariableDeclaration()
          ? path.get('declarations').map(declaration => Object.keys(declaration.getOuterBindingIdentifiers()))
          : [programBindings];
        layout.push([position,path.node.type,path.node.kind ?? null,groups]);
      }
    },
    Identifier(path) {
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
