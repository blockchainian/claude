// Ported from wakaru v1.14.0 UnInfinity (Apache-2.0).
export default {
  name: 'UnInfinity',
  globals: ['Infinity'],
  visitor(context) {
    return {
      BinaryExpression: {
        exit(path) {
          const { operator, left, right } = path.node;
          if (operator !== '/') return;
          if (right.type !== 'NumericLiteral' || right.value !== 0) return;
          const positive = left.type === 'NumericLiteral' && left.value === 1;
          const negative = left.type === 'UnaryExpression' && left.operator === '-'
            && left.argument.type === 'NumericLiteral' && left.argument.value === 1;
          if (!positive && !negative) return;
          if (!context.canIntroduceGlobal(path, 'Infinity')) return;
          context.replace(path, negative ? '-Infinity' : 'Infinity');
        },
      },
    };
  },
};
