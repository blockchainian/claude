// Ported from wakaru v1.14.0 UnTypeofStrict (Apache-2.0).
import { comparisonOperator, isTypeof } from './un-typeof-source.mjs';

export default {
  name: 'UnTypeofStrict',
  visitor(context) {
    return {
      BinaryExpression: {
        exit(path) {
          const node = path.node;
          if (node.operator !== '==' && node.operator !== '!=') return;
          if (!((isTypeof(node.left) && node.right.type === 'StringLiteral')
            || (isTypeof(node.right) && node.left.type === 'StringLiteral'))) return;
          context.replace(comparisonOperator(context.source, node), `${node.operator}=`);
        },
      },
    };
  },
};
