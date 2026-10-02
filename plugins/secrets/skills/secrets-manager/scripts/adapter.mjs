// External adapters access engine code only through this kit.
import { readFileSync } from 'node:fs';
import { isAbsolute } from 'node:path';
import { pathToFileURL } from 'node:url';
import * as page from './page-helpers.mjs';
import * as debug from './debug.mjs';
import * as restriction from './restriction.mjs';
import * as emailOtp from './email-otp.mjs';
import * as store from './store.mjs';
import * as config from './config.mjs';
import { configureBlocklist } from './traffic.mjs';

/**
 * @typedef {{db: object, cred: object, opts: object, io: object}} ByEmailContext
 * @typedef {{status: 'ok'|'error', alias?: string, detail?: string}} ByEmailResult
 * @typedef {{db: object, email: string, session: object, opts: object, io: object}} VerifyContext
 * @typedef {object} Adapter
 * @property {string} name Table and CLI target, [a-z0-9_].
 * @property {string} domain Registrable domain and session scope.
 * @property {string} startUrl Login entry URL.
 * @property {string[]} entryTexts Logged-out entry labels.
 * @property {(page: object) => Promise<void>} signIn Open Google OAuth (popup or same tab).
 * @property {(page: object) => Promise<boolean>} ready App session present and live.
 * @property {(url: string) => boolean} [signedInUrl]
 * @property {number} [attempts] Default 1.
 * @property {(ctx: ByEmailContext) => Promise<ByEmailResult>} [byEmail]
 * @property {(ctx: VerifyContext) => Promise<'active'|'restricted'|'expired'>} [verify]
 * @property {{envVar: string, token: (session: object) => string|null}} [exportEnv]
 * @property {string[]} [blockedHosts] Host wildcards.
 * @property {string[]} [blockedWebSockets] WebSocket URL wildcards.
 */
export const kit = {
  ...page, page, debug, restriction, emailOtp, store, config,
  withProfile: async (...args) => (await import('./login.mjs')).withProfile(...args),
  mintAppPassword: async (...args) => (await import('./login.mjs')).mintAppPassword(...args),
};
// aliasFor is synchronous and does not need the browser runtime.
kit.aliasFor = (baseEmail, tag) => {
  if (typeof tag !== 'string' || !tag) throw new Error('alias tag is required');
  const [local, domain] = baseEmail.split('@');
  return `${local.split('+', 1)[0]}+${tag}@${domain}`;
};

export function validateAdapter(adapter) {
  if (!adapter || typeof adapter !== 'object' || Array.isArray(adapter)) throw new Error('adapter must be an object');
  for (const field of ['name', 'domain', 'startUrl']) {
    if (typeof adapter[field] !== 'string' || !adapter[field]) throw new Error(`adapter ${field} is required`);
  }
  const table = store.toAppSlug(adapter.name);
  if (!/^[a-z0-9_]+$/.test(adapter.name) || adapter.name !== table ||
      ['google', 'x', 'tiktok'].includes(table) || table.startsWith('sqlite_')) {
    throw new Error(`bad adapter name: ${adapter.name}`);
  }
  if (!Array.isArray(adapter.entryTexts) || !adapter.entryTexts.length || adapter.entryTexts.some(t => typeof t !== 'string' || !t)) throw new Error('adapter entryTexts must be nonempty strings');
  for (const hook of ['signIn', 'ready']) if (typeof adapter[hook] !== 'function') throw new Error(`adapter ${hook} must be a function`);
  for (const hook of ['signedInUrl', 'byEmail', 'verify']) if (adapter[hook] !== undefined && typeof adapter[hook] !== 'function') throw new Error(`adapter ${hook} must be a function`);
  if (adapter.attempts !== undefined && (!Number.isInteger(adapter.attempts) || adapter.attempts < 1)) throw new Error('adapter attempts must be a positive integer');
  if (adapter.exportEnv !== undefined && (!adapter.exportEnv || !/^[A-Z_][A-Z0-9_]*$/.test(adapter.exportEnv.envVar) || typeof adapter.exportEnv.token !== 'function')) throw new Error('adapter exportEnv needs envVar and token function');
  for (const field of ['blockedHosts', 'blockedWebSockets']) if (adapter[field] !== undefined && (!Array.isArray(adapter[field]) || adapter[field].some(t => typeof t !== 'string' || !t))) throw new Error(`adapter ${field} must be strings`);
  return adapter;
}

export async function loadAdapters({ configPath = config.configJsonPath(), env = process.env, paths } = {}) {
  if (paths === undefined) {
    if (env.SECRETS_MANAGER_ADAPTERS !== undefined) paths = env.SECRETS_MANAGER_ADAPTERS ? env.SECRETS_MANAGER_ADAPTERS.split(':') : [];
    else {
      let text;
      try { text = readFileSync(configPath, 'utf8'); } catch (e) { if (e.code !== 'ENOENT') throw e; }
      paths = text === undefined ? [] : JSON.parse(text).adapters;
    }
  }
  if (!Array.isArray(paths)) throw new Error(`${configPath}: adapters must be an array`);
  const adapters = [];
  for (const path of paths) {
    try {
      if (typeof path !== 'string' || !isAbsolute(path)) throw new Error('adapter path must be absolute');
      const module = await import(pathToFileURL(path).href);
      if (typeof module.default !== 'function') throw new Error('default export must be a factory');
      const entries = await module.default(kit);
      if (!Array.isArray(entries)) throw new Error('factory must return an array');
      for (const entry of entries) {
        validateAdapter(entry);
        if (adapters.some(a => store.toAppSlug(a.name) === store.toAppSlug(entry.name))) throw new Error(`duplicate adapter name: ${entry.name}`);
        adapters.push(entry);
      }
    } catch (e) { throw new Error(`${path}: ${e.message}`, { cause: e }); }
  }
  configureBlocklist(adapters);
  return adapters;
}
