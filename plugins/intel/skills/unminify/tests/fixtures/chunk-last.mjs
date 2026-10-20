// chunk-xrybxn8g.js; headlessCloudAttach.js lines 600-629.
async function us(e) {
  let n = await Mze(e, 1, { reportFeatureHealth: !1 });
  if (n === null) throw Error("the session history could not be read");
  let s = n.newestSequenceNum ?? n.events.at(-1)?.sequenceNum;
  if (s) return s;
  if (n.events.length > 0 || n.hasMore || (n.droppedRows ?? 0) > 0)
    throw Error("the newest event could not be read");
  return 0;
}
var Zi = 500;
export {
  sQn,
  jWt,
  rqo,
  A2r,
  R2r,
  x2r,
  iQn,
  aQn,
  lQn,
  dQn,
  cQn,
  uQn,
  pQn,
  fQn,
  Hei,
  P2r,
  I2r,
  Dei,
};
