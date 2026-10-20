// Ported from wakaru v1.14.0 FlipComparisons / eval_utils (Apache-2.0).
import { isLexicalIdentifier } from '../../identifiers.mjs';

const unwrap = node => node.type === 'ParenthesizedExpression' ? unwrap(node.expression) : node;
const constants = new Set(['undefined', 'NaN', 'Infinity']);
const operators = new Map([['==', '=='], ['!=', '!='], ['===', '==='], ['!==', '!=='],
  ['<', '>'], ['>', '<'], ['<=', '>='], ['>=', '<=']]);

function evalSource(node) {
  if (node?.type === 'StringLiteral') return node.value;
  if (node?.type !== 'CallExpression' || node.arguments.length !== 2) return null;
  const member = node.callee, [pattern, prefix] = node.arguments;
  if (member.type === 'MemberExpression' && !member.computed && member.property.name === 'replace'
    && member.object.type === 'StringLiteral' && member.object.value === 'quire'
    && pattern.type === 'RegExpLiteral' && pattern.pattern === '^'
    && prefix.type === 'StringLiteral' && prefix.value === 're') return 'require';
  return null;
}

function isRequireRead(source) {
  return /^require\s*(?:\(\s*(?:"[^"\\\r\n]*"|'[^'\\\r\n]*')\s*\))?(?:\s*\.\s*[$A-Z_a-z][$\w]*)*\s*;?$/.test(source.trim());
}

function needsExpressionParens(node) {
  if (node.type === 'BinaryExpression' || node.type === 'LogicalExpression') return needsExpressionParens(node.left);
  if (node.type === 'MemberExpression' || node.type === 'OptionalMemberExpression') return needsExpressionParens(node.object);
  if (node.type === 'CallExpression' || node.type === 'OptionalCallExpression') return needsExpressionParens(node.callee);
  if (node.type === 'TaggedTemplateExpression') return needsExpressionParens(node.tag);
  if (node.type === 'UpdateExpression' && !node.prefix) return needsExpressionParens(node.argument);
  return ['ObjectExpression', 'FunctionExpression', 'ClassExpression'].includes(node.type);
}

export default {
  name: 'FlipComparisons',
  visitor(context) {
    let dynamicScope = false, requireReadEval = false, requireBinding = false;
    let edits = [];
    function globalConstant(node, path) {
      if (node.type === 'UnaryExpression' && node.operator === '-') {
        node = node.argument;
        if (node.type !== 'Identifier' || node.name !== 'Infinity') return false;
      }
      return node.type === 'Identifier' && constants.has(node.name)
        && !path.scope.getBinding(node.name);
    }
    function literalLike(node, path) {
      if (['NullLiteral', 'BooleanLiteral', 'NumericLiteral', 'StringLiteral', 'BigIntLiteral'].includes(node.type)) return true;
      if (node.type === 'TemplateLiteral') return node.expressions.length === 0;
      if (globalConstant(node, path)) return true;
      if (node.type !== 'UnaryExpression') return false;
      const argument = node.argument;
      if (node.operator === 'void') return argument.type === 'NumericLiteral';
      if (node.operator === '!') return argument.type === 'NumericLiteral' && [0, 1].includes(argument.value);
      return node.operator === '-' && ['NumericLiteral', 'BigIntLiteral'].includes(argument.type);
    }
    function rendered(node) {
      let result = '', offset = node.start;
      for (const edit of edits.filter(edit => edit.start >= node.start && edit.end <= node.end).sort((a, b) => a.start - b.start)) {
        result += context.source.slice(offset, edit.start) + edit.text;
        offset = edit.end;
      }
      return result + context.source.slice(offset, node.end);
    }
    return {
      Program: {
        enter(path) {
          path.traverse({
            BindingIdentifier(binding) {
              if (binding.node.name === 'require' && isLexicalIdentifier(binding)) requireBinding = true;
            },
          });
          path.traverse({
            WithStatement() { dynamicScope = true; },
            CallExpression(call) {
              const callee = unwrap(call.node.callee);
              if (callee.type !== 'Identifier' || callee.name !== 'eval') return;
              const args = call.node.arguments;
              const source = args.some(argument => argument.type === 'SpreadElement') ? null : evalSource(args[0]);
              if (source !== null && isRequireRead(source)) requireReadEval = true;
              else dynamicScope = true;
              call.skip();
            },
          });
          dynamicScope ||= requireReadEval && requireBinding;
        },
        exit() {
          for (const edit of edits) context.replace(edit, edit.text);
        },
      },
      BinaryExpression: {
        exit(path) {
          const { left, right, operator } = path.node;
          if (!operators.has(operator)) return;
          // Fragment visitors cannot prove whether another slice binds a global constant.
          if ((context.fragment || dynamicScope) && (globalConstant(left, path) || globalConstant(right, path))) return;
          if (!literalLike(left, path) || literalLike(right, path)) return;
          const leftText = rendered(left);
          const rightText = needsExpressionParens(right) ? `(${rendered(right)})` : rendered(right);
          // Keep comments and whitespace between operands while replacing only the operator token.
          const middle = context.source.slice(left.end, right.start).replace(
            /\/\*[\s\S]*?\*\/|\/\/[^\r\n]*|===|!==|==|!=|<=|>=|<|>/g,
            token => token === operator ? operators.get(operator) : token,
          );
          edits = edits.filter(edit => edit.end <= path.node.start || edit.start >= path.node.end);
          edits.push({ start: path.node.start, end: path.node.end, text: rightText + middle + leftText });
        },
      },
    };
  },
};
