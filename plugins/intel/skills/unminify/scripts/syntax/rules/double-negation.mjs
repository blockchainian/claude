// Ported from wakaru v1.14.0 UnDoubleNegation (Apache-2.0).
function ambiguousStart(node) {
  while (node) {
    if (['ObjectExpression', 'FunctionExpression', 'ClassExpression'].includes(node.type)) return true;
    if (node.type === 'MemberExpression' || node.type === 'OptionalMemberExpression') {
      if (node.computed && node.object.type === 'Identifier' && node.object.name === 'let') return true;
      node = node.object;
    } else if (node.type === 'CallExpression' || node.type === 'OptionalCallExpression') {
      node = node.callee;
    } else if (node.type === 'TaggedTemplateExpression') {
      node = node.tag;
    } else if (node.type === 'UpdateExpression' && !node.prefix) {
      node = node.argument;
    } else return false;
  }
  return false;
}

export default {
  name: 'UnDoubleNegation',
  visitor(context) {
    function stripDoubleBang(path) {
      const outer = path.node.test;
      if (outer?.type !== 'UnaryExpression' || outer.operator !== '!') return;
      const inner = outer.argument;
      if (inner.type !== 'UnaryExpression' || inner.operator !== '!') return;
      // Conditional tests can start statements or arrow bodies after removing the bangs.
      const parenthesize = path.isConditionalExpression() && ambiguousStart(inner.argument);
      // Keep operand ranges intact so nested conditions and comments survive.
      context.replace({ start: outer.start, end: outer.start + 1 }, parenthesize ? '(' : '');
      context.replace({ start: inner.start, end: inner.start + 1 }, '');
      if (parenthesize) context.replace({ start: inner.argument.end, end: inner.argument.end }, ')');
    }
    return {
      'IfStatement|WhileStatement|DoWhileStatement|ForStatement|ConditionalExpression': {
        exit: stripDoubleBang,
      },
    };
  },
};
