// Ported from wakaru v1.14.0 UnTypeof (Apache-2.0).
import { comparisonOperator, isTypeof } from './un-typeof-source.mjs';

const isU = node => node.type === 'StringLiteral' && node.value === 'u';

export default {
  name: 'UnTypeof',
  visitor(context) {
    let edits = [];
    return {
      BinaryExpression: {
        exit(path) {
          const node = path.node;
          if (node.operator !== '<' && node.operator !== '>') return;
          const forward = isTypeof(node.left) && isU(node.right);
          const reverse = isU(node.left) && isTypeof(node.right);
          if (!forward && !reverse) return;
          const operand = forward ? node.left : node.right;
          const operator = comparisonOperator(context.source, node);
          const equality = forward === (node.operator === '>') ? '===' : '!==';
          // Fold child edits into this replacement before sending disjoint intervals.
          const children = edits.filter(edit => edit.start >= operand.start && edit.end <= operand.end)
            .sort((a, b) => a.start - b.start);
          let text = '', offset = operand.start;
          for (const edit of children) {
            text += context.source.slice(offset, edit.start) + edit.text;
            offset = edit.end;
          }
          text += context.source.slice(offset, operand.end);
          text += context.source.slice(node.left.end, operator.start) + equality
            + context.source.slice(operator.end, node.right.start) + '"undefined"';
          // Equality has lower precedence than the original relational comparison.
          if (path.parentPath.isBinaryExpression()) text = `(${text})`;
          edits = edits.filter(edit => edit.start < node.start || edit.end > node.end);
          edits.push({ start: node.start, end: node.end, text });
        },
      },
      Program: {
        exit() {
          for (const edit of edits) context.replace(edit, edit.text);
        },
      },
    };
  },
};
