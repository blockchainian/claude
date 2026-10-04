// ABOUTME: Installs the Camoufox release compatible with the pinned JavaScript launcher.
// ABOUTME: Uses camoufox-js's own cache, extraction, permissions, and version.json layout.
import { readFileSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// v156.0.1-beta.34 removed navigator.product and other properties that camoufox-js 0.12.0 sets.
const PINNED_BROWSER = { version: "152.0.4", release: "beta.30" };
const pinnedVersion = `${PINNED_BROWSER.version}-${PINNED_BROWSER.release}`;

export function assetName(platform = process.platform, arch = process.arch) {
  const os = { darwin: "mac", linux: "lin", win32: "win" }[platform];
  const cpu = { x64: "x86_64", arm64: "arm64", ia32: "i686" }[arch];
  const supported = { mac: ["arm64", "x86_64"], lin: ["arm64", "x86_64"], win: ["i686", "x86_64"] };
  if (!supported[os]?.includes(cpu)) throw new Error(`Unsupported Camoufox platform: ${platform}/${arch}`);
  return `camoufox-${pinnedVersion}-${os}.${cpu}.zip`;
}

function installedVersion(dir) {
  try {
    return JSON.parse(readFileSync(join(dir, "version.json"), "utf8"));
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

function matchesPin(version) {
  return version?.version === PINNED_BROWSER.version && version?.release === PINNED_BROWSER.release;
}

export function browserReport(dir) {
  const version = installedVersion(dir);
  if (!version) return `camoufox: missing (pin: ${pinnedVersion})`;
  const installed = `${version.version}-${version.release}`;
  return matchesPin(version)
    ? `camoufox: ${installed} (matches pin)`
    : `camoufox: ${installed} (mismatch; pin: ${pinnedVersion})`;
}

export async function ensurePinnedBrowser(dir, install) {
  if (matchesPin(installedVersion(dir))) return;
  await install();
  if (!matchesPin(installedVersion(dir))) throw new Error("Installed Camoufox does not match the pin");
}

async function main() {
  const { INSTALL_DIR, CamoufoxFetcher } = await import("camoufox-js/dist/pkgman.js");
  if (process.argv[2] !== "--check") {
    const asset = assetName();
    class PinnedFetcher extends CamoufoxFetcher {
      async init() {} // The release is fixed; never look up the latest one.
      get version() { return PINNED_BROWSER.version; }
      get release() { return PINNED_BROWSER.release; }
      get url() { return `https://github.com/daijro/camoufox/releases/download/v${pinnedVersion}/${asset}`; }
    }
    await ensurePinnedBrowser(INSTALL_DIR, () => new PinnedFetcher().install());
  }
  console.log(browserReport(INSTALL_DIR));
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  main().catch((error) => { console.error(`camoufox: ${error.message}`); process.exitCode = 1; });
}
