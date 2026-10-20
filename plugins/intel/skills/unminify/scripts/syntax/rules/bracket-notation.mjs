// Ported from wakaru v1.14.0 UnBracketNotation (Apache-2.0).
const identifierName = /^[$_\p{ID_Start}][$\u200c\u200d\p{ID_Continue}]*$/u;
const wordEnd = /[$\u200c\u200d\p{ID_Continue}]$/u;
const openingBracket = /\[(?:\s|\/\*[\s\S]*?\*\/|\/\/[^\r\n\u2028\u2029]*)*$/u;
const closingBracket = /^(?:\s|\/\*[\s\S]*?\*\/|\/\/[^\r\n\u2028\u2029]*)*\]/u;

function normalizedDecimal(value) {
  if (!/^[0-9]+(?:\.[0-9]+)?$/.test(value)) return false;
  const number = Number(value);
  return (number === 0 || (number >= 1e-6 && number < 1e21)) && String(number) === value;
}

export default {
  name: 'UnBracketNotation',
  visitor(context) {
    function removeBrackets(node, key, member) {
      const prefixStart = member ? node.object.end : node.start;
      const prefix = context.source.slice(prefixStart, key.start);
      const open = openingBracket.exec(prefix);
      const close = closingBracket.exec(context.source.slice(key.end, node.end));
      if (!open || !close) throw new Error('UnBracketNotation: missing computed key delimiters');
      const start = prefixStart + open.index;
      let separator = '';
      if (member) {
        if (!node.optional) {
          const rawObject = context.source.slice(node.object.start, node.object.end);
          separator = node.object.type === 'NumericLiteral' && /^[0-9][0-9_]*$/.test(rawObject) ? ' .' : '.';
        }
      } else if (wordEnd.test(context.source.slice(0, start))) {
        // Compact modifiers such as get["key"] still need a token boundary.
        separator = ' ';
      }
      context.replace({ start, end: start + 1 }, separator);
      const end = key.end + close[0].length;
      context.replace({ start: end - 1, end }, '');
    }

    function rewrite(path) {
      const node = path.node;
      const member = path.isMemberExpression() || path.isOptionalMemberExpression();
      // SWC visits super property expressions separately from member expressions.
      if (member && node.object.type === 'Super') return;
      const key = member ? node.property : node.key;
      if (key.type !== 'StringLiteral' || (member && !node.computed)) return;
      if (!member && node.computed && ['__proto__', 'constructor', 'prototype'].includes(key.value)) return;
      const numeric = normalizedDecimal(key.value);
      if (!numeric && !identifierName.test(key.value)) return;
      context.replace(key, key.value);
      if (node.computed && (!member || !numeric)) removeBrackets(node, key, member);
    }

    return {
      'MemberExpression|OptionalMemberExpression|ObjectProperty|ObjectMethod|ClassProperty|ClassMethod|ClassAccessorProperty': rewrite,
    };
  },
};
