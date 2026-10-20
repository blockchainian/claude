// chunk-xrybxn8g.js; attachServe.js lines 22-33.
function qs(e, n) {
  switch (n.kind) {
    case "never":
      return !0;
    case "unknown":
      return !1;
    case "before": {
      let s = Date.parse(e);
      return Number.isFinite(s) && s - 86400000 > n.before.getTime();
    }
  }
}
