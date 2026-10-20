// Ported from wakaru v1.14.0 rule registry (Apache-2.0).
// Keep numeric positions from wakaru; repeat an implementation at another order.
// Each entry dynamically loads one rule, so registration requires just one line.
export const registry = [
  { order: 5, name: 'RemoveVoid', module: './rules/remove-void.mjs' },
  { order: 6, name: 'UnminifyBooleans', module: './rules/unminify-booleans.mjs' },
];
export const rules = await Promise.all([...registry].sort((a,b)=>a.order-b.order).map(async entry =>
  ({ ...(await import(entry.module)).default, name: entry.name })));
