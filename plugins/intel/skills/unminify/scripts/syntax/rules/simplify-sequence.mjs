// Ported from wakaru v1.14.0 SimplifySequence (Apache-2.0).
import { checkSyntaxRewrite } from '../checks.mjs';
import { isLexicalIdentifier } from '../../identifiers.mjs';

const unwrap = node => node?.type === 'ParenthesizedExpression' ? unwrap(node.expression) : node;
function sequence(node) {
  const expr = unwrap(node);
  return expr?.type === 'SequenceExpression' ? [expr.expressions.slice(0, -1), expr.expressions.at(-1)] : [[], expr];
}
function eligible(path) {
  return (path.listKey === 'body' && (path.parentPath.isProgram() || path.parentPath.isBlockStatement()))
    || (path.listKey === 'consequent' && path.parentPath.isSwitchCase())
    || (path.parentPath.isIfStatement() && ['consequent', 'alternate'].includes(path.key) && eligible(path.parentPath));
}
function statementHead(node) {
  switch (node.type) {
    case 'MemberExpression': case 'OptionalMemberExpression': return statementHead(node.object);
    case 'CallExpression': case 'OptionalCallExpression': return statementHead(node.callee);
    case 'TaggedTemplateExpression': return statementHead(node.tag);
    case 'BinaryExpression': case 'LogicalExpression': case 'AssignmentExpression': return statementHead(node.left);
    case 'ConditionalExpression': return statementHead(node.test);
    case 'SequenceExpression': return statementHead(node.expressions[0]);
    default: return node;
  }
}

export default {
  name: 'SimplifySequence',
  visitor(context) {
    const edits = [];
    let withPresent = false;
    // Compose child replacements before replacing a containing statement.
    function render(node, replacements = edits) {
      let result = '', offset = node.start;
      for (const edit of replacements.filter(e => e.start >= node.start && e.end <= node.end).sort((a, b) => a.start - b.start)) {
        result += context.source.slice(offset, edit.start) + edit.text;
        offset = edit.end;
      }
      return result + context.source.slice(offset, node.end);
    }
    function replace(node, text) {
      if (text === render(node)) return;
      for (let i = edits.length - 1; i >= 0; i--) {
        if (edits[i].start >= node.start && edits[i].end <= node.end) edits.splice(i, 1);
      }
      edits.push({ start: node.start, end: node.end, text });
    }
    const expression = node => {
      const text = render(node);
      const head = statementHead(node);
      // Expression statements must not become declarations or blocks.
      return ['ObjectExpression', 'ObjectPattern', 'FunctionExpression', 'ClassExpression'].includes(head.type)
        || (head.type === 'Identifier' && head.name === 'let')
        ? `(${text});` : `${text};`;
    };
    function withField(node, field, value) {
      const own = edits.filter(e => !(e.start >= field.start && e.end <= field.end));
      return render(node, [...own, { start: field.start, end: field.end, text: value }]);
    }
    function lexicalPrefixBlocked(prefix, declarationPath, futureNames) {
      const bindings = new Set();
      for (const declaration of declarationPath.get('declarations')) {
        for (const name of Object.keys(declaration.getBindingIdentifiers())) bindings.add(declarationPath.scope.getBinding(name));
      }
      let blocked = false;
      for (const expr of prefix) {
        const inspect = p => {
          if (p.isIdentifier() && isLexicalIdentifier(p)
            && (futureNames.has(p.node.name) || bindings.has(p.scope.getBinding(p.node.name)))) blocked = true;
          if (p.isWithStatement() || (p.isCallExpression() && unwrap(p.node.callee)?.type === 'Identifier'
            && unwrap(p.node.callee).name === 'eval')) blocked = true;
        };
        // A prefix can itself be the reference or eval call being checked.
        const initPaths = declarationPath.get('declarations').flatMap(d => d.get('init')?.node ? [d.get('init')] : []);
        for (const init of initPaths) {
          if (expr.start < init.node.start || expr.end > init.node.end) continue;
          init.traverse({ enter(p) { if (p.node.start >= expr.start && p.node.end <= expr.end) inspect(p); } });
          if (init.node === expr) inspect(init);
        }
      }
      return blocked;
    }
    function declaration(path, inFor = false) {
      const node = path.node, prefix = [], pending = [];
      if (!['var', 'let', 'const'].includes(node.kind)) return null;
      const names = new Set(Object.keys(path.getBindingIdentifiers()));
      let changed = !inFor && node.declarations.length > 1;
      for (const d of node.declarations) {
        for (const name of Object.keys(path.get('declarations').find(p => p.node === d).getBindingIdentifiers())) names.delete(name);
        let [pre, last] = d.init ? sequence(d.init) : [[], null];
        if (inFor && pre.length && (pre.some(p => unwrap(p).type === 'StringLiteral')
          || (node.kind !== 'var' && (pending.length || lexicalPrefixBlocked(pre, path, names))))) pre = [];
        if (pre.length) {
          changed = true;
          if (inFor && node.kind === 'var' && pending.length) prefix.push(`${node.kind} ${pending.splice(0).join(', ')};`);
          prefix.push(...pre.map(expression));
        }
        const text = pre.length ? `${render(d.id)} = ${inFor ? `(${render(last)})` : render(last)}` : render(d);
        if (inFor) pending.push(text);
        else prefix.push(`${node.kind} ${text};`);
      }
      return changed ? { prefix, header: `${node.kind} ${pending.join(', ')}` } : null;
    }
    function split(path) {
      const node = path.node;
      if (path.isExpressionStatement()) {
        const expr = node.expression;
        if (!withPresent && expr.type === 'AssignmentExpression' && expr.left.type === 'MemberExpression') {
          const member = expr.left, inner = unwrap(member.object);
          if (inner.type === 'AssignmentExpression' && inner.left.type === 'Identifier') {
            return [expression(inner), `${withField(expr, member.object, inner.left.name)};`];
          }
        }
        const seq = expr.type === 'ParenthesizedExpression' ? expr.expression : expr;
        if (seq.type === 'SequenceExpression') return seq.expressions.map(expression);
        if (expr.type === 'YieldExpression' && expr.argument?.type === 'SequenceExpression') {
          const [first, ...rest] = expr.argument.expressions;
          return [`yield${expr.delegate ? '*' : ''} ${render(first)};`, ...rest.map(expression)];
        }
      } else if (path.isReturnStatement() || path.isThrowStatement()) {
        if (!node.argument) return null;
        const [pre, last] = sequence(node.argument);
        if (pre.length) return [...pre.map(expression), `${path.isReturnStatement() ? 'return' : 'throw'} ${render(last)};`];
      } else if (path.isIfStatement() || path.isSwitchStatement()) {
        const field = node.test ?? node.discriminant, [pre, last] = sequence(field);
        if (pre.length) return [...pre.map(expression), withField(node, field, render(last))];
      } else if (path.isVariableDeclaration()) {
        return declaration(path)?.prefix ?? null;
      } else if (path.isForStatement()) {
        if (!node.init) return null;
        if (node.init.type === 'VariableDeclaration') {
          const decl = declaration(path.get('init'), true);
          if (decl) return [...decl.prefix, withField(node, node.init, decl.header)];
        } else {
          const [pre, last] = sequence(node.init);
          const call = unwrap(last);
          if (!pre.length && !(call.type === 'CallExpression' && unwrap(call.callee).type === 'Identifier')) return null;
          const keep = last.type === 'AssignmentExpression';
          return [...pre.map(expression), ...(!keep ? [expression(last)] : []), withField(node, node.init, keep ? `(${render(last)})` : '')];
        }
      } else if (path.isForInStatement() || path.isForOfStatement()) {
        if (node.left.type === 'VariableDeclaration' && ['let', 'const'].includes(node.left.kind)) return null;
        const [pre, last] = sequence(node.right);
        if (pre.length) return [...pre.map(expression), withField(node, node.right, render(last))];
      }
      return null;
    }
    function preservesFragment(source) {
      const reason = checkSyntaxRewrite(context.source, source, { fragment: true });
      if (!reason) return true;
      if (['FRAGMENT_BINDINGS_CHANGED', 'FRAGMENT_DECLARATIONS_CHANGED'].includes(reason)) return false;
      throw new Error(`SimplifySequence: ${reason}`);
    }
    return {
      Program: {
        enter(path) { path.traverse({ WithStatement() { withPresent = true; } }); },
        exit() {
          if (!edits.length) return;
          if (!context.fragment) {
            for (const edit of edits) context.replace(edit, edit.text);
            return;
          }
          // The shared checker proves fragment binding kinds and evaluation order.
          // Check the batch first; only a rejected batch needs per-statement checks.
          if (preservesFragment(render({ start: 0, end: context.source.length }))) {
            for (const edit of edits) context.replace(edit, edit.text);
          } else {
            for (const edit of edits) {
              const candidate = context.source.slice(0, edit.start) + edit.text + context.source.slice(edit.end);
              if (preservesFragment(candidate)) context.replace(edit, edit.text);
            }
          }
        },
      },
      Statement: {
        exit(path) {
          if (!eligible(path)) return;
          const result = split(path);
          if (!result) return;
          const lineStart = context.source.lastIndexOf('\n', path.node.start - 1) + 1;
          const indent = context.source.slice(lineStart, path.node.start).match(/^[\t ]*/)[0];
          const newline = context.source.includes('\r\n') ? '\r\n' : '\n';
          const text = result.join(newline + indent);
          replace(path.node, path.parentPath.isIfStatement() && result.length > 1
            ? `{${newline}${indent}${text}${newline}${indent}}` : text);
        },
      },
    };
  },
};
