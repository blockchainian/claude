// ABOUTME: Scopes an exported browser storage state to one app's domain.
// ABOUTME: Pure; shared by the engine's session export and the adapter kit.

// Split a Playwright storageState down to one app's host. Returns {cookies, local_storage}. A
// cookie is kept when the app host and the cookie's domain are the same registrable domain —
// either way round, so both a parent-domain cookie (.example.com) and a subdomain one
// (api.example.com) count; a localStorage origin is kept when its host is the app domain or
// below it. `domain` must be the registrable base (e.g. example.com).
export function filterState(state, domain) {
  const cookies = (state.cookies ?? []).filter(
    (c) => within(domain, c.domain ?? "") || within(c.domain ?? "", domain),
  );
  const localStorage = [];
  for (const origin of state.origins ?? []) {
    let host = "";
    try {
      host = new URL(origin.origin ?? "").hostname;
    } catch {
      /* not a URL */
    }
    if (within(host, domain)) localStorage.push(origin);
  }
  return { cookies, local_storage: localStorage };
}

// True if `host` is `base` or a sub-domain of it.
function within(host, base) {
  host = host.replace(/^\.+/, "").toLowerCase();
  base = base.replace(/^\.+/, "").toLowerCase();
  return host === base || host.endsWith("." + base);
}
