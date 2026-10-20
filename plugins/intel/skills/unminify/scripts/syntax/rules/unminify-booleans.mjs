// Ported from wakaru 1.14.0 UnminifyBooleans (Apache-2.0).
export default {
  name: 'UnminifyBooleans',
  visitor(context) {
    return {
      UnaryExpression: {
        exit(path) {
          const { operator, argument } = path.node;
          if (operator !== '!' || argument.type !== 'NumericLiteral') return;
          if (argument.value === 0) context.replace(path, 'true');
          else if (argument.value === 1) context.replace(path, 'false');
        },
      },
    };
  },
};
