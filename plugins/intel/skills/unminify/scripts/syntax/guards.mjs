// Ported from wakaru v1.14.0 eval_utils/rename_utils (Apache-2.0).
import { parse } from '@babel/parser';
import { isLexicalIdentifier } from '../identifiers.mjs';
import traverseModule from '@babel/traverse';
export const traverse = traverseModule.default ?? traverseModule;
export const parseSyntax = source => parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true, createParenthesizedExpressions: true });
const unwrap = node => node?.type === 'ParenthesizedExpression' ? unwrap(node.expression) : node;
function evalSource(node) {
  if (node?.type === 'StringLiteral') return node.value;
  // Bundlers hide require from static analyzers with this exact spelling.
  if (node?.type !== 'CallExpression' || node.arguments.length !== 2) return null;
  const member = node.callee, [pattern, prefix] = node.arguments;
  if (member.type !== 'MemberExpression' || member.computed || member.property.name !== 'replace'
    || member.object.type !== 'StringLiteral' || member.object.value !== 'quire'
    || pattern.type !== 'RegExpLiteral' || pattern.pattern !== '^'
    || prefix.type !== 'StringLiteral' || prefix.value !== 're') return null;
  return 'require';
}
export function globalGuard(ast, fragment) {
  const declared = new Set(), evals = [];
  let withStatement = false, unknownEval = false;
  traverse(ast, {
    BindingIdentifier(path) { if (isLexicalIdentifier(path)) declared.add(path.node.name); },
    WithStatement() { withStatement = true; },
    CallExpression(path) {
      if (unwrap(path.node.callee)?.type !== 'Identifier' || unwrap(path.node.callee).name !== 'eval') return;
      const args = path.node.arguments;
      if (!args.length) return;
      const source = args.some(arg => arg.type === 'SpreadElement') ? null : evalSource(args[0]);
      if (source === null) unknownEval = true;
      else evals.push(source);
    },
  });
  return name => !fragment && !declared.has(name) && !withStatement && !unknownEval
    && !evals.some(source => new RegExp(`(^|[^a-zA-Z0-9_$])${name}($|[^a-zA-Z0-9_$])`).test(source));
}
