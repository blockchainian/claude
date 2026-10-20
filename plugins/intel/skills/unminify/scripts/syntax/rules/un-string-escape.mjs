// Ported from wakaru v1.14.0 UnStringEscape (Apache-2.0).
const invisibleRanges = [
  [0xad, 0xad], [0x34f, 0x34f], [0x61c, 0x61c], [0x115f, 0x1160],
  [0x17b4, 0x17b5], [0x180b, 0x180f], [0x200b, 0x200f], [0x202a, 0x202e],
  [0x2060, 0x206f], [0x3164, 0x3164], [0xfe00, 0xfe0f], [0xfeff, 0xfeff],
  [0xffa0, 0xffa0], [0xfff0, 0xffff], [0xe000, 0xf8ff], [0x1bca0, 0x1bca3],
  [0x1d173, 0x1d17a], [0x323b0, 0x10ffff],
];

function parseEscape(raw) {
  const match = /^\\(?:x([0-9a-fA-F]{2})|u\{([0-9a-fA-F]+)\}|u([0-9a-fA-F]{4}))/.exec(raw);
  if (!match) return null;
  let codePoint = Number.parseInt(match[1] ?? match[2] ?? match[3], 16);
  if (codePoint > 0x10ffff) return null;
  let length = match[0].length;
  if (match[3] && codePoint >= 0xd800 && codePoint <= 0xdbff) {
    const lowMatch = /^\\u([0-9a-fA-F]{4})/.exec(raw.slice(length));
    if (lowMatch) {
      const low = Number.parseInt(lowMatch[1], 16);
      if (low >= 0xdc00 && low <= 0xdfff) {
        codePoint = 0x10000 + ((codePoint - 0xd800) << 10) + low - 0xdc00;
        length += lowMatch[0].length;
      }
    }
  }
  return { codePoint, length };
}

function isPrintable(codePoint) {
  return codePoint > 0x7f
    && !(codePoint >= 0xd800 && codePoint <= 0xdfff)
    && !/[\p{Cc}\p{White_Space}]/u.test(String.fromCodePoint(codePoint))
    && !invisibleRanges.some(([start, end]) => codePoint >= start && codePoint <= end);
}

function decodePrintableEscapes(raw) {
  if (!raw.includes('\\u') && !raw.includes('\\x')) return raw;
  let output = '';
  for (let i = 0; i < raw.length;) {
    if (raw[i] !== '\\') {
      output += raw[i++];
      continue;
    }
    const escape = parseEscape(raw.slice(i));
    if (escape) {
      const { codePoint, length } = escape;
      // Surrogate halves beside a hyphen can be regex range endpoints.
      const rangePair = length === 12 && (raw[i - 1] === '-' || raw[i + length] === '-');
      output += isPrintable(codePoint) && !rangePair
        ? String.fromCodePoint(codePoint) : raw.slice(i, i + length);
      i += length;
    } else {
      // Copy both characters of other escapes, including an escaped backslash.
      output += raw.slice(i, i + 2);
      i += 2;
    }
  }
  return output;
}

export default {
  name: 'UnStringEscape',
  visitor(context) {
    return {
      'StringLiteral|DirectiveLiteral'(path) {
        if (path.parentPath.isJSXAttribute()) return;
        const raw = context.source.slice(path.node.start, path.node.end);
        const decoded = decodePrintableEscapes(raw);
        if (decoded !== raw) context.replace(path, decoded);
      },
    };
  },
};
