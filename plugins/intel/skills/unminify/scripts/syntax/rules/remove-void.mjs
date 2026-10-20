// Ported from wakaru 1.14.0 RemoveVoid (Apache-2.0).
export default {
  name: 'RemoveVoid',
  globals: ['undefined'],
  visitor(context) {
    return {
      UnaryExpression: {
        enter(path) {
          // Wakaru keeps the complete operand of delete untouched.
          if (path.node.operator === 'delete') path.skip();
        },
        exit(path) {
          if (path.node.operator !== 'void') return;
          let argument = path.node.argument;
          while (argument.type === 'ParenthesizedExpression') argument = argument.expression;
          if (argument.type !== 'NumericLiteral') return;
          if (!context.canIntroduceGlobal(path, 'undefined')) return;
          context.replace(path, 'undefined');
        },
      },
    };
  },
};
