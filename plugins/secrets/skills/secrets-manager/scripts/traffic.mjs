// ABOUTME: Blocks the requests a login never needs so they never reach the residential proxy:
// ABOUTME: the apps' high-volume data hosts (a blackhole list) plus images, media and fonts.

// Host wildcards (`*` matches any run of characters). These are the apps' market-data, telemetry
// and asset hosts: they carry the bulk of a page's traffic and none of the login.
export const BLOCKED_HOSTS = [];
export const BLOCKED_WEBSOCKETS = [];

// Resource types a login never needs.
export const BLOCKED_RESOURCE_TYPES = new Set(["image", "media", "font"]);

const toRegExp = (pattern) => new RegExp(`^${pattern.split("*").map((s) => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*")}$`, "i");
let HOST_RES = BLOCKED_HOSTS.map(toRegExp);

export function isBlockedHost(host) {
  return HOST_RES.some((re) => re.test(host));
}

// Google's own sign-in assets — including the reCAPTCHA image challenge's tiles
// (www.google.com/recaptcha/…) and gstatic images — are low-volume and REQUIRED for a human to
// solve the challenge, so images/media/fonts from these hosts must pass even though the blocklist
// otherwise drops all of them. Keeps the blocklist installable during a headed run.
export function isLoginAssetHost(host) {
  return /(^|\.)google\.com$/i.test(host) || /(^|\.)gstatic\.com$/i.test(host);
}

// True when a request should be aborted: a blackholed host, or a resource type the login never
// needs (unless it is a Google sign-in asset). Documents, scripts, stylesheets and XHR/fetch to
// other hosts pass.
export function shouldBlock(url, resourceType) {
  let host;
  try {
    host = new URL(url).hostname;
  } catch {
    return false;
  }
  if (isBlockedHost(host)) return true;
  if (BLOCKED_RESOURCE_TYPES.has(resourceType)) return !isLoginAssetHost(host);
  return false;
}

// Install the blocklist on a browser context: matching HTTP requests are aborted, matching
// WebSockets are never connected.
export async function installBlocklist(context) {
  await context.route("**/*", (route) => {
    const request = route.request();
    if (shouldBlock(request.url(), request.resourceType())) return route.abort();
    return route.continue();
  });
  for (const pattern of BLOCKED_WEBSOCKETS) await context.routeWebSocket(pattern, () => {});
}

export function configureBlocklist(adapters) {
  BLOCKED_HOSTS.splice(0, BLOCKED_HOSTS.length, ...adapters.flatMap(a => a.blockedHosts ?? []));
  BLOCKED_WEBSOCKETS.splice(0, BLOCKED_WEBSOCKETS.length, ...adapters.flatMap(a => a.blockedWebSockets ?? []));
  HOST_RES = BLOCKED_HOSTS.map(toRegExp);
}
