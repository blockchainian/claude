// Source helpers for wakaru v1.14.0 UnTypeof / UnTypeofStrict (Apache-2.0).
export const isTypeof = node => node.type === 'UnaryExpression' && node.operator === 'typeof';

export function comparisonOperator(source, node) {
  const gap = source.slice(node.left.end, node.right.start);
  const trivia = gap.match(/^(?:\s|\/\*[\s\S]*?\*\/|\/\/[^\r\n\u2028\u2029]*)*/)[0];
  const start = node.left.end + trivia.length;
  const end = start + node.operator.length;
  if (source.slice(start, end) !== node.operator) throw new Error('missing typeof comparison operator');
  return { start, end };
}
