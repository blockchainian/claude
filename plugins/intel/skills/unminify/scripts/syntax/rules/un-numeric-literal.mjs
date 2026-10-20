// Ported from wakaru v1.14.0 UnNumericLiteral (Apache-2.0).
function decimalSpelling(text) {
  if (!text.includes('e')) return text;
  const [mantissa, exponent] = text.split('e');
  const digits = mantissa.replace('.', '');
  const point = (mantissa.includes('.') ? mantissa.indexOf('.') : mantissa.length) + Number(exponent);
  if (point <= 0) return `0.${'0'.repeat(-point)}${digits}`;
  if (point >= digits.length) return digits + '0'.repeat(point - digits.length);
  return `${digits.slice(0, point)}.${digits.slice(point)}`;
}

export default {
  name: 'UnNumericLiteral',
  visitor(context) {
    return {
      NumericLiteral(path) {
        const { node, parent } = path;
        // SWC visits numeric expressions, leaving noncomputed property names alone.
        if (path.key === 'key' && !parent.computed) return;
        if (!Number.isFinite(node.value)) return;
        let text = String(node.value);
        const raw = context.source.slice(node.start, node.end);
        // Rust's Display uses decimal notation; matching raw is retained by Wakaru.
        if (raw === decimalSpelling(text)) return;
        // An adjacent member dot needs a second dot after a decimal integer.
        if (path.key === 'object' && parent.type === 'MemberExpression'
          && !parent.computed && context.source[node.end] === '.' && /^\d+$/.test(text)) {
          text += '.';
        }
        if (text !== raw) context.replace(path, text);
      },
    };
  },
};
