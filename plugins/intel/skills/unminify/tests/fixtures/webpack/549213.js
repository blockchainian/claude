// webpack module 549213 from _next/static/chunks/util-vendors-cf2e2532dffdd98f.js
webpackModule(549213, (e, t, n) => {
  "use strict";

  n.d(t, {
    _: () => a
  });
  var r = n(409870),
    i = n(392759);
  function a(e, t) {
    var n = "object" == typeof t;
    return new Promise(function (a, o) {
      var u = new i.Ms({
        next: function (e) {
          a(e), u.unsubscribe();
        },
        error: o,
        complete: function () {
          n ? a(t.defaultValue) : o(new r.G());
        }
      });
      e.subscribe(u);
    });
  }
});
