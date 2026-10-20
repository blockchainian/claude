// Ported from wakaru v1.14.0 rule registry (Apache-2.0).
// Keep numeric positions from wakaru; repeat an implementation at another order.
// Each entry dynamically loads one rule, so registration requires just one line.
export const registry = [
  { order: 2, name: 'SimplifySequence', module: './rules/simplify-sequence.mjs' },
  { order: 3, name: 'FlipComparisons', module: './rules/flip-comparisons.mjs' },
  { order: 4, name: 'UnTypeofStrict', module: './rules/un-typeof-strict.mjs' },
  { order: 5, name: 'RemoveVoid', module: './rules/remove-void.mjs' },
  { order: 6, name: 'UnminifyBooleans', module: './rules/unminify-booleans.mjs' },
  { order: 7, name: 'UnDoubleNegation', module: './rules/double-negation.mjs' },
  { order: 8, name: 'UnInfinity', module: './rules/un-infinity.mjs' },
  { order: 9, name: 'UnIndirectCall', module: './rules/un-indirect-call.mjs' },
  { order: 10, name: 'UnTypeof', module: './rules/un-typeof.mjs' },
  { order: 11, name: 'UnNumericLiteral', module: './rules/un-numeric-literal.mjs' },
  { order: 12, name: 'UnStringEscape', module: './rules/un-string-escape.mjs' },
  { order: 13, name: 'UnBracketNotation', module: './rules/bracket-notation.mjs' },
  { order: 24, name: 'SimplifySequence2', module: './rules/simplify-sequence.mjs' },
];
export const rules = await Promise.all([...registry].sort((a,b)=>a.order-b.order).map(async entry =>
  ({ ...(await import(entry.module)).default, name: entry.name })));
