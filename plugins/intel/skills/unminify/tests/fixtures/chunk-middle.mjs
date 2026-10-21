// Synthetic middle slice: no module boundary; a sibling calls Kv.
function Kv(a) {
  return Math.trunc(a) === a && a >= 3;
}
function Mt(b) {
  return Nt.includes(b);
}
