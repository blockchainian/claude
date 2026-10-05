// Command-line entry for the local secrets store and external adapters.
import { readFileSync, readdirSync, realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { loadAdapters, OPTIONS } from "./adapter.mjs";
import * as config from "./config.mjs";
import { runWithConcurrency } from "./concurrency.mjs";
import { loadCredentials, parseLine, setAppPassword, setTotpSecret } from "./credentials.mjs";
import * as sms from "./sms-otp.mjs";
import * as store from "./store.mjs";
import { BUILTIN_CHECKS, CHECK_RESULTS, nextStatus } from "./verify.mjs";
import { NeedsHuman } from "./errors.mjs";
// --- credential file parsing -------------------------------------------------

// X accounts arrive from a vendor in a few colon-separated shapes (6 or 8 fields, and the
// auth-token/TOTP columns appear in either order). Rather than a fixed position, fields are keyed
// by shape: the first four are always username:password:email:email_password, and the TOTP secret
// is the base32 (16-char) field among the rest. Extra vendor columns (a long token, a UUID, a live
// auth_token) are ignored — the flow logs in fresh from password + TOTP.
const isTotpSecret = (v) => /^[A-Z2-7]{16}$/.test(v);
const isAuthToken = (v) => /^[0-9a-f]{40}$/.test(v);

export function parseXVendorLine(line) {
  const parts = line.split(":");
  if (parts.length < 5) throw new Error(`expected >=5 fields, got ${parts.length}`);
  const [username, password, email, email_password] = parts;
  if (!email.includes("@")) throw new Error("field 3 is not an email address");
  const rest = parts.slice(4);
  const totp_secret = rest.find(isTotpSecret);
  if (!totp_secret) throw new Error("no base32 TOTP secret in the trailing fields");
  // The vendor also ships a live 40-hex auth_token per line; capture it so we can try it before a
  // password login. It may be undefined for a shape that omits it. (No ct0 is present in the file.)
  const auth_token = rest.find(isAuthToken);
  return { username, password, email, email_password, totp_secret, auth_token };
}

// TikTok accounts arrive as `username:password:email:email_password:profile_url`. Only the first
// four fields are kept; the profile url repeats the username. The email pair is optional: a third
// field that is not an address (the profile url of a line without one) is not taken for it.
export function parseTiktokLine(line) {
  const [username, password, email, email_password] = line.split(":").map((part) => part.trim());
  if (!username || !password) throw new Error("line has no username or password");
  const hasEmail = Boolean(email?.includes("@"));
  return { username, password, email: hasEmail ? email : null, email_password: (hasEmail && email_password) || null };
}

export function parseFile(text, decode) {
  const accounts = [];
  const errors = [];
  text.split(/\r?\n/).forEach((raw, i) => {
    const line = raw.trim();
    if (!line || line.startsWith("#")) return;
    try {
      const account = decode(line);
      if (account) accounts.push(account);
    } catch (e) {
      errors.push(`line ${i + 1}: ${e.message}`);
    }
  });
  return { accounts, errors };
}

// --- shared helpers ------------------------------------------------------------

let adapters = [];
let APP_ADAPTERS = [];
const getAdapter = name => {
  const adapter = adapters.find(a => a.name === name);
  if (!adapter) throw new Error(`unknown adapter ${name}; loaded: ${APP_ADAPTERS.join(", ") || "(none)"}`);
  return adapter;
};
// Login tables keyed by username instead of a Google email.
const USERNAME_TABLES = {
  x: { label: "X", setStatus: store.setXStatus },
  tiktok: { label: "TikTok", setStatus: store.setTiktokStatus },
};
let SESSION_TABLES = [...APP_ADAPTERS, ...Object.keys(USERNAME_TABLES)];

function assertOneOf(what, value, allowed) {
  if (!allowed.includes(value)) throw new Error(`${what} must be one of ${allowed.join(", ")}`);
}

function listCredentialFiles(app) {
  try {
    return readdirSync(config.credentialsDir(app))
      .filter((f) => f.endsWith(".txt"))
      .sort()
      .map((f) => join(config.credentialsDir(app), f));
  } catch {
    return [];
  }
}

function pick(items, key, opts) {
  let out = opts.select.length ? items.filter((it) => opts.select.includes(key(it))) : items;
  if (opts.limit) out = out.slice(0, opts.limit);
  return out;
}

// Google credential lines, upserted into the google table, narrowed by --select/--limit.
function googleAccounts(db, opts) {
  const creds = loadCredentials(config.credentialsDir("google"));
  for (const cred of creds) store.upsertAccount(db, cred.email, cred.password, cred.totp_secret, cred.app_password);
  return pick(creds, (c) => c.email, opts);
}

// A headed run is an interactive one: the flow drives everything it can on its own (including renting
// a HeroSMS number for a phone step) and only pauses for the person when it hits a node the script
// cannot pass — a reCAPTCHA image challenge — clearing it in the visible window. A headless run has
// no window to clear, so such a node escalates instead. There is no per-challenge flag.
const browserOpts = (opts) => ({ headed: opts.headed, rotate: opts["rotate-proxy"] });

// A Google sign-in (login google/<app>, setup-2fa) runs headed by default: a person at the window
// clears the CAPTCHAs, since the headless vision solver's misses get accounts banned. `--headless`
// opts out.
export const googleBrowserOpts = (opts) => ({ headed: !opts.headless, rotate: opts["rotate-proxy"] });

// --- import ------------------------------------------------------------------

const IMPORTERS = {
  google: { decode: parseLine, upsert: (db, a) => store.upsertAccount(db, a.email, a.password, a.totp_secret, a.app_password) },
  x: { decode: parseXVendorLine, upsert: store.upsertX },
  tiktok: { decode: parseTiktokLine, upsert: store.upsertTiktok },
};

function runImport(db, opts, io) {
  const [app] = opts.positional;
  assertOneOf("import target", app, Object.keys(IMPORTERS));
  const { decode, upsert } = IMPORTERS[app];
  const files = opts.positional.length > 1 ? opts.positional.slice(1) : listCredentialFiles(app);
  if (files.length === 0) throw new Error(`no files given and none found in ${config.credentialsDir(app)}`);
  for (const file of files) {
    const { accounts, errors } = parseFile(readFileSync(file, "utf8"), decode);
    for (const account of accounts) upsert(db, account);
    io.log(`${file}: imported ${accounts.length} ${app} account(s)`);
    for (const error of errors) io.log(`  skipped ${error}`);
  }
  return 0;
}

// --- login -------------------------------------------------------------------

// Apps that need a fresh session: never seen, or explicitly marked expired. Skips active and ready
// (still good), restricted and escalated (won't retry until cleared).
export function needsRefresh(db, app, email) {
  const session = store.getSession(db, app, email);
  return session === null || session.status === store.STATUS_EXPIRED;
}

async function runLogin(db, opts, io) {
  const [target] = opts.positional;
  assertOneOf("login target", target, ["google", "x", "tiktok", ...APP_ADAPTERS]);
  if (opts["by-email"]) return loginByEmail(db, target, opts, io);
  if (target === "x") return loginX(db, opts, io);
  if (target === "tiktok") return loginTiktok(db, opts, io);
  return loginGoogleBacked(db, target, opts, io);
}

// `login google` signs every selected account into Google (no app). `login <app>` signs into
// Google when needed, then into the app with "Sign in with Google" and stores the session; by
// default only accounts whose app session is missing or expired, `--all` for every account.
// Why to skip an account before launching a browser, or null to attempt it. `restricted` is an app
// ban, never retried. `escalated` needs a human (a Google challenge): `login google` retries it so
// a headed run can clear it (a success then clears the status), other targets still skip it.
export function loginSkipReason(status, target) {
  if (status === store.STATUS_RESTRICTED) return `account status ${status}`;
  if (status === store.STATUS_ESCALATED && target !== "google") return `account status ${status}`;
  return null;
}

async function loginGoogleBacked(db, target, opts, io) {
  const creds = googleAccounts(db, opts);
  if (creds.length === 0) {
    io.error(`no credential files in ${config.credentialsDir("google")}`);
    return 1;
  }
  const adapters = target === "google" ? [] : [getAdapter(target)];
  const login = await import("./login.mjs");
  // Drive up to --concurrency accounts at once (default 1 = the old sequential behavior). Each
  // account has its own profile, sticky proxy exit and DB row, so the flows never collide; a headed
  // run opens that many browser windows and a held-open debug window occupies its slot until closed.
  const outcomes = await runWithConcurrency(creds, opts.concurrency, (cred) =>
    loginOneGoogleBacked(db, target, adapters, login, opts, io, cred),
  );
  return outcomes.some((failed) => failed) ? 1 : 0;
}

// Logs one account into `target`, returning true on failure. Sets the account's status from the
// error class; a plain failure is logged but not marked so one account never halts the rest.
async function loginOneGoogleBacked(db, target, adapters, login, opts, io, cred) {
  const account = store.getAccount(db, cred.email);
  const skip = loginSkipReason(account.status, target);
  if (skip) {
    io.log(`skip ${cred.email}: ${skip}`);
    return false;
  }
  if (target !== "google" && store.getSession(db, target, cred.email)?.status === store.STATUS_RESTRICTED) {
    io.log(`skip ${cred.email}: ${target} status restricted`);
    return false;
  }
  if (target !== "google" && !opts.all && !needsRefresh(db, target, cred.email)) {
    io.log(`${cred.email}: ${target} up to date, skipping`);
    return false;
  }
  io.log(`${cred.email}: logging in to ${target}`);
  try {
    await login.runAccount(db, cred, adapters, googleBrowserOpts(opts));
    io.log(`  ${cred.email}: ok`);
    return false;
  } catch (e) {
    if (e instanceof login.AppRestricted) {
      store.recordSessionStatus(db, e.app, cred.email, store.STATUS_RESTRICTED);
      io.error(`  ${e.app} restricted (won't retry): ${e.message}`);
    } else if (e instanceof login.Restricted) {
      store.setAccountStatus(db, cred.email, store.STATUS_RESTRICTED);
      io.error(`  restricted (won't retry): ${e.message}`);
    } else if (e instanceof login.Expired) {
      store.setAccountStatus(db, cred.email, store.STATUS_EXPIRED);
      io.error(`  expired (retry later): ${e.message}`);
    } else if (e instanceof login.NeedsHuman) {
      store.setAccountStatus(db, cred.email, store.STATUS_ESCALATED);
      io.error(`  needs human: ${e.message}`);
    } else {
      io.error(`  ${cred.email} failed: ${e.message}`);
    }
    return true;
  }
}

// Log every pending row of a username-keyed table in: `pending` lists the rows (all of them with
// force), `loadLogin` lazily imports the browser flow that logs one row in and returns its outcome.
async function loginByUsername(db, opts, io, { table, pending, loadLogin }) {
  const rows = pick(pending(db, { force: opts.all || opts.select.length > 0 }), (r) => r.username, opts);
  if (rows.length === 0) {
    io.log(`no ${USERNAME_TABLES[table].label} accounts need login`);
    return 0;
  }
  const loginAccount = await loadLogin();
  const outcomes = await runWithConcurrency(rows, opts.concurrency, async (row) => {
    try {
      const outcome = await loginAccount(db, row, browserOpts(opts));
      io.log(`${row.username}: ${outcome}`);
      return outcome !== "ok";
    } catch (e) {
      USERNAME_TABLES[table].setStatus(db, row.username, store.STATUS_EXPIRED);
      io.error(`  ${row.username} failed: ${e.message}`); // one account's failure must not halt the rest
      return true;
    }
  });
  return outcomes.some(Boolean) ? 1 : 0;
}

const loginX = (db, opts, io) =>
  loginByUsername(db, opts, io, {
    table: "x",
    pending: store.getPendingX,
    loadLogin: async () => (await import("./x-login.mjs")).loginXAccount,
  });

const loginTiktok = (db, opts, io) =>
  loginByUsername(db, opts, io, {
    table: "tiktok",
    pending: store.getPendingTiktok,
    loadLogin: async () => (await import("./tiktok-login.mjs")).loginTiktokAccount,
  });

async function loginByEmail(db, target, opts, io) {
  const adapter = getAdapter(target);
  if (!adapter.byEmail) throw new Error(`${target} has no byEmail hook`);
  const creds = googleAccounts(db, opts).filter(c => !c.email.split('@')[0].includes('+'));
  if (!creds.length) { io.error('no base accounts to log in'); return 1; }
  const results = await runWithConcurrency(creds, opts.concurrency, async cred => {
    try {
      const result = await adapter.byEmail({ db, cred, opts, io });
      if (!result || !['ok', 'error'].includes(result.status)) throw new Error('invalid byEmail result');
      if (result.status === 'ok') {
        if (result.alias) store.upsertAccount(db, result.alias, cred.password, cred.totp_secret, cred.app_password);
        io.log(`${result.alias || cred.email}: ok (${result.detail || ''})`);
        return false;
      }
      io.error(`${cred.email}: ${result.detail || 'byEmail failed'}`);
    } catch (e) { io.error(`${cred.email}: ${e.message}`); }
    return true;
  });
  return results.some(Boolean) ? 1 : 0;
}

// --- setup-2fa ---------------------------------------------------------------

// Provision the second factor for Google accounts that only have email:password: add an
// authenticator (scraping its TOTP secret), turn on 2-Step Verification, and mint a Gmail app
// password, writing the TOTP secret and app password back to the credential file so the account
// matches the full email:password:totp_secret:app_password shape. Requires a live Google session
// per account (`login google` first); a logged-out account is reported and skipped without marking
// it escalated. Skips accounts already carrying both a TOTP secret and an app password unless
// --all; the three steps are independent, so a resumed run only does what is missing. The app
// password is written to the file, never printed.
async function runSetup2fa(db, opts, io) {
  const creds = googleAccounts(db, opts);
  if (creds.length === 0) {
    io.error(`no credential files in ${config.credentialsDir("google")}`);
    return 1;
  }
  const login = await import("./login.mjs");
  const dir = config.credentialsDir("google");
  const outcomes = await runWithConcurrency(creds, opts.concurrency, async (cred) => {
    const account = store.getAccount(db, cred.email);
    if (account.status === store.STATUS_RESTRICTED || account.status === store.STATUS_ESCALATED) {
      io.log(`skip ${cred.email}: account status ${account.status}`);
      return false;
    }
    if (!opts.all && cred.totp_secret && cred.app_password) {
      io.log(`${cred.email}: 2FA already provisioned, skipping`);
      return false;
    }
    io.log(`${cred.email}: setting up 2FA`);
    try {
      // 1. Authenticator + TOTP secret (skip when one is already on file).
      if (opts.all || !cred.totp_secret) {
        const secret = await login.enrollAuthenticator(cred, googleBrowserOpts(opts));
        if (!secret) {
          io.error(`  ${cred.email}: could not enroll an authenticator (see debug capture)`);
          return true;
        }
        // Save the secret before 2-Step is on: once it is, the account cannot log in without it,
        // and the app-password re-auth may itself demand a code.
        if (!setTotpSecret(dir, cred.email, secret)) {
          io.error(`  ${cred.email}: enrolled but no credential file carries this email to save the secret`);
          return true;
        }
        cred.totp_secret = secret;
        store.upsertAccount(db, cred.email, cred.password, cred.totp_secret, cred.app_password);
        io.log(`  ${cred.email}: authenticator enrolled, TOTP secret saved`);
      }
      // 2. Turn 2-Step Verification on (idempotent — a re-run on an already-on account is a no-op).
      if (!(await login.turnOnTwoStep(cred, googleBrowserOpts(opts)))) {
        io.error(`  ${cred.email}: 2-Step Verification did not turn on (see debug capture)`);
        return true;
      }
      io.log(`  ${cred.email}: 2-Step Verification on`);
      // 3. Gmail app password (skip when one is already on file).
      if (opts.all || !cred.app_password) {
        const appPw = await login.mintAppPassword(cred, { ...googleBrowserOpts(opts), name: "secrets-manager" });
        if (!appPw) {
          io.error(`  ${cred.email}: could not mint an app password (see debug capture)`);
          return true;
        }
        setAppPassword(dir, cred.email, appPw);
        cred.app_password = appPw;
        store.upsertAccount(db, cred.email, cred.password, cred.totp_secret, cred.app_password);
        io.log(`  ${cred.email}: app password saved to the credential file`);
      }
      io.log(`  ${cred.email}: ok`);
      return false;
    } catch (e) {
      if (e instanceof login.NotLoggedIn) {
        io.error(`  ${cred.email}: not logged in — run \`login google\` first`); // not a challenge; leave status
      } else if (e instanceof login.NeedsHuman) {
        store.setAccountStatus(db, cred.email, store.STATUS_ESCALATED);
        io.error(`  needs human: ${e.message}`);
      } else {
        io.error(`  ${cred.email} failed: ${e.message}`); // one account's failure must not halt the rest
      }
      return true;
    }
  });
  return outcomes.some(Boolean) ? 1 : 0;
}

// --- sms ---------------------------------------------------------------------

const SMS_MAX_PRICE_CAP = 0.1; // never pay more than this per number, whatever --max-price says
const SMS_DEFAULT_MAX_PRICE = 0.05;

// Rent Google-verification phone numbers from HeroSMS and read the SMS they receive. `balance`
// prints the account balance; `prices` lists the countries offering a Google (`go`) number at or
// under --max-price, cheapest first; `number` rents one (cheapest affordable country by default,
// or --country N), waits for the code, then closes the activation — or cancels for a refund if no
// code arrives. `number` spends money, so it is a dry run unless --yes is given, and --max-price is
// clamped to $0.10. Needs HERO_SMS_API_KEY in ~/.config/secrets-manager/.env.
async function runSms(_db, opts, io) {
  const [verb] = opts.positional;
  assertOneOf("sms verb", verb, ["balance", "prices", "number"]);
  const maxPrice = Math.min(
    opts["max-price"] !== undefined ? Number(opts["max-price"]) : SMS_DEFAULT_MAX_PRICE,
    SMS_MAX_PRICE_CAP,
  );
  try {
    if (verb === "balance") {
      io.log(`balance: $${(await sms.getBalance()).toFixed(2)}`);
      return 0;
    }
    if (verb === "prices") {
      const rows = sms.affordableCountries(await sms.getPrices({ country: opts.country }), sms.GOOGLE_SERVICE, maxPrice, config.SMS_COUNTRY_BLACKLIST);
      if (rows.length === 0) {
        io.log(`no Google (go) numbers at or under $${maxPrice} — raise --max-price (cap $${SMS_MAX_PRICE_CAP})`);
        return 0;
      }
      io.log(`Google (go) numbers at or under $${maxPrice}, cheapest first:`);
      for (const row of rows) io.log(`  country ${row.country}\t$${row.cost}\t${row.count} in stock`);
      return 0;
    }
    // verb === "number": choose a country, quote the price, and (with --yes) rent + poll + close.
    let { country } = opts;
    let quote;
    if (country === undefined) {
      const rows = sms.affordableCountries(await sms.getPrices({}), sms.GOOGLE_SERVICE, maxPrice, config.SMS_COUNTRY_BLACKLIST);
      if (rows.length === 0) {
        io.error(`no Google number at or under $${maxPrice}; raise --max-price (cap $${SMS_MAX_PRICE_CAP}) or pass --country`);
        return 1;
      }
      ({ country, cost: quote } = rows[0]);
    }
    io.log(`about to rent a Google number in country ${country}${quote !== undefined ? ` (~$${quote})` : ""}, max $${maxPrice}`);
    if (!opts.yes) {
      io.log("dry run — pass --yes to actually buy (spends money)");
      return 0;
    }
    const { activationId, phone } = await sms.requestNumber({ country, maxPrice });
    io.log(`rented ${phone} (activation ${activationId}); waiting up to 5 min for the SMS…`);
    const result = await sms.pollCode(activationId, { timeoutS: 300 });
    if (result.state === "ok") {
      await sms.setStatus(activationId, sms.STATUS_COMPLETE);
      io.log(`CODE ${result.code} — phone ${phone}, activation ${activationId} (closed)`);
      return 0;
    }
    await sms.setStatus(activationId, sms.STATUS_CANCEL);
    io.error(`no code (${result.state}); cancelled activation ${activationId} — refunded`);
    return 1;
  } catch (e) {
    io.error(e.message);
    return 1;
  }
}

// --- verify ------------------------------------------------------------------

// An adapter app's sessions as a verify target: one row per Google account holding a session, checked
// by the adapter's own verify hook.
function adapterCheck(target) {
  const adapter = getAdapter(target);
  if (!adapter.verify) throw new Error(`${target} has no verify hook`);
  return {
    label: target,
    rows: db => store.listAccounts(db).flatMap(a => {
      const session = store.getSession(db, target, a.email);
      return session ? [{ email: a.email, status: session.status }] : [];
    }),
    id: row => row.email,
    setStatus: (db, email, status) => store.setSessionStatus(db, target, email, status),
    check: ({ db, row, opts, io }) => adapter.verify({ db, email: row.email, session: store.getSession(db, target, row.email), opts, io }),
    results: [store.STATUS_ACTIVE, store.STATUS_RESTRICTED, store.STATUS_EXPIRED],
  };
}

// Checks each selected account of a builtin target (google, x, tiktok) or an adapter app and writes
// back its status (see nextStatus); a check that throws or returns something else leaves it as it was.
async function runVerify(db, opts, io) {
  const [target] = opts.positional;
  const spec = BUILTIN_CHECKS[target] ?? adapterCheck(target);
  const allowed = spec.results ?? CHECK_RESULTS;
  const rows = pick(spec.rows(db).filter(r => [store.STATUS_ACTIVE, store.STATUS_READY].includes(r.status) || opts.all), spec.id, opts);
  if (!rows.length) { io.log(`no ${spec.label} active accounts to check`); return 0; }
  const results = await runWithConcurrency(rows, opts.concurrency, async row => {
    const id = spec.id(row);
    try {
      const status = await spec.check({ db, row, opts, io });
      if (!allowed.includes(status)) throw new Error(`invalid verify result ${status}`);
      const stored = nextStatus(row.status, status);
      spec.setStatus(db, id, stored);
      io.log(stored === status ? `${id}: ${status}` : `${id}: ${status} (kept ${stored})`);
      return false;
    } catch (e) { io.error(`${id}: ${e.message}`); return true; }
  });
  return results.some(Boolean) ? 1 : 0;
}

// --- setup -------------------------------------------------------------------

// Setup needs a live app session. Ready rows only join an explicit --all rerun;
// NeedsHuman escalates the session, while other failures leave its status unchanged.
async function runSetup(db, opts, io) {
  const [app] = opts.positional;
  const adapter = getAdapter(app);
  if (!adapter.setup) throw new Error(`${app} has no setup hook`);
  const sessions = store.listAccounts(db).flatMap(account => {
    const session = store.getSession(db, app, account.email);
    return session ? [session] : [];
  });
  const rows = pick(sessions.filter(session => session.status === store.STATUS_ACTIVE ||
    (opts.all && session.status === store.STATUS_READY)), session => session.email, opts);
  if (!rows.length) { io.log(`no ${app} active sessions to set up`); return 0; }
  const results = await runWithConcurrency(rows, opts.concurrency, async session => {
    const email = session.email;
    try {
      const result = await adapter.setup({ db, email, session, opts, io });
      if (!result || typeof result !== "object" || Array.isArray(result)) throw new Error("invalid setup result");
      const { summary, state } = result;
      if (typeof summary !== "string" || !summary.trim() || /[\r\n\u2028\u2029]/.test(summary)) throw new Error("invalid setup summary");
      store.saveSetupState(db, app, email, state);
      io.log(`${email}: ${summary}`);
      return false;
    } catch (e) {
      if (e instanceof NeedsHuman) {
        store.setSessionStatus(db, app, email, store.STATUS_ESCALATED);
        io.error(`${email}: escalated: ${e.message}`);
      } else io.error(`${email}: ${e.message}`);
      return true;
    }
  });
  return results.some(Boolean) ? 1 : 0;
}

// --- whoami ------------------------------------------------------------------

// Resolve the caller-supplied credential without opening or changing the local store.
async function runWhoami(opts, io) {
  const [app] = opts.positional;
  if (app === "x") {
    const { whoamiX } = await import("./x-verify.mjs");
    const identity = await whoamiX({ credential: opts.select[0] });
    io.log(opts.json ? JSON.stringify({ app, username: identity.username }) : `@${identity.username}`);
    return 0;
  }
  const adapter = getAdapter(app);
  if (!adapter.whoami) throw new Error(`${app} has no whoami hook`);
  const identity = await adapter.whoami({ credential: opts.select[0] });
  if (!identity || typeof identity.email !== "string" || !/^\S+@\S+\.\S+$/.test(identity.email)) {
    throw new Error(`${app} returned an invalid whoami result`);
  }
  io.log(opts.json ? JSON.stringify({ app, email: identity.email }) : identity.email);
  return 0;
}

// --- export ------------------------------------------------------------------

function runExport(db, opts, io) {
  const [app] = opts.positional;
  const adapter = getAdapter(app);
  if (!adapter.credentials) throw new Error(`${app} has no credentials hook`);
  for (const account of pick(store.listAccounts(db), (a) => a.email, opts)) {
    const session = store.getSession(db, app, account.email);
    if (!session || ![store.STATUS_ACTIVE, store.STATUS_READY].includes(session.status)) continue;
    const fields = adapter.credentials(session);
    if (fields === null) io.error(`${account.email}\tmissing`);
    else io.log(JSON.stringify({ app, email: account.email, ...fields }));
  }
  return 0;
}

// --- get / set-status / list -------------------------------------------------

// The accounts --select names, required by the commands that read or change named accounts.
function selected(command, opts) {
  if (!opts.select.length) throw new Error(`${command} needs --select ID`);
  return opts.select;
}

// Print each selected account's stored session; a missing one is reported and makes the exit 1.
function runGet(db, opts, io) {
  const [app] = opts.positional;
  assertOneOf("app", app, [...new Set([...APP_ADAPTERS, ...store.listApps(db), ...Object.keys(USERNAME_TABLES)])]);
  let code = 0;
  for (const id of selected("get", opts)) {
    const session = app in USERNAME_TABLES ? db.prepare(`SELECT * FROM ${app} WHERE username = ?`).get(id) : store.getSession(db, app, id);
    if (!session) {
      io.error(`no ${app} session for ${id}`);
      code = 1;
      continue;
    }
    io.log(JSON.stringify(session, null, 2));
  }
  return code;
}

// Set each selected account's status; a missing one is reported and makes the exit 1.
function runSetStatus(db, opts, io) {
  const [app, status] = opts.positional;
  assertOneOf("app", app, [...new Set([...APP_ADAPTERS, ...store.listApps(db), ...Object.keys(USERNAME_TABLES), "google"])]);
  assertOneOf("status", status, [...store.STATUSES].sort());
  let code = 0;
  for (const id of selected("set-status", opts)) {
    try {
      if (app === "google") store.setAccountStatus(db, id, status);
      else if (app in USERNAME_TABLES) USERNAME_TABLES[app].setStatus(db, id, status);
      else store.setSessionStatus(db, app, id, status);
    } catch (e) {
      io.error(e.message);
      code = 1;
      continue;
    }
    io.log(`${id} @ ${app}: ${status}`);
  }
  return code;
}

function runList(db, opts, io) {
  const rows = store.listAccounts(db).map((account) => ({
    email: account.email,
    status: account.status,
    last_login_at: account.last_login_at,
    apps: store.sessionsForAccount(db, account.email),
  }));
  if (opts.json) {
    io.log(JSON.stringify(rows, null, 2));
    return 0;
  }
  if (rows.length === 0) {
    io.log("no accounts yet");
    return 0;
  }
  for (const row of rows) {
    const appsDesc = Object.entries(row.apps).map(([a, s]) => `${a}=${s}`).join(", ") || "(no apps)";
    io.log(`${row.email.padEnd(36)} ${row.status.padEnd(11)} login:${row.last_login_at || "-"}  ${appsDesc}`);
  }
  return 0;
}

// --- CLI ---------------------------------------------------------------------

const COMMANDS = {
  whoami: { positional: [1, 1] },
  validate: { positional: [1, Infinity] },
  import: { run: runImport, positional: [1, Infinity] },
  login: { run: runLogin, positional: [1, 1] },
  verify: { run: runVerify, positional: [1, 1] },
  setup: { run: runSetup, positional: [1, 1] },
  "setup-2fa": { run: runSetup2fa, positional: [0, 0] },
  sms: { run: runSms, positional: [1, 1] },
  export: { run: runExport, positional: [1, 1] },
  get: { run: runGet, positional: [1, 1] },
  "set-status": { run: runSetStatus, positional: [2, 2] },
  list: { run: runList, positional: [0, 0] },
};

const USAGE = `Usage: secrets-manager <command> [options]
  import <google|x|tiktok> [file...]
  login <google|app> [--select ID]... [--all] [--limit N] [--concurrency N] [--headless] [--rotate-proxy]
  login <x|tiktok> [--select ID]... [--all] [--limit N] [--concurrency N] [--headed] [--rotate-proxy]
  login <app> --by-email [--mint-app-password] [--select EMAIL]... [--headed]
  verify <google|x|tiktok|app> [--select ID]... [--all] [--concurrency N] [--headed] [--rotate-proxy]
  setup <app> [--select EMAIL]... [--all] [--concurrency N] [--headed] [--rotate-proxy] [app flags]
  setup-2fa [--select EMAIL]... [--all] [--headless] [--limit N] [--concurrency N] [--rotate-proxy]
  sms <balance|prices|number> [--country N] [--max-price X] [--yes]
  whoami <x|app> --select CREDENTIAL [--json]
  export <app> [--select EMAIL]...
  get <app> --select ID...
  set-status <app> <active|ready|expired|restricted|escalated> --select ID...
  list [--json]`;

function setupUsage(adapter) {
  const flags = Object.entries(adapter?.setupFlags ?? {}).map(([name, flag]) =>
    `  --${name}${flag.type === "string" ? " VALUE" : ""}  ${flag.description}`);
  return [USAGE.split("\n").find(line => line.startsWith("  setup <app>")),
    ...(flags.length ? [`${adapter.name} flags:`, ...flags] : [])].join("\n");
}

export function parseCli(argv, setupAdapter) {
  const [command, ...rest] = argv;
  const spec = COMMANDS[command];
  if (!spec) throw new Error(USAGE);
  let options = OPTIONS;
  if (command === "setup") {
    const flags = Object.fromEntries(Object.entries(setupAdapter?.setupFlags ?? {}).map(([name, flag]) => [name, { type: flag.type }]));
    options = { ...OPTIONS, help: { type: "boolean" }, ...flags };
  }
  let values, positionals;
  try {
    ({ values, positionals } = parseArgs({ args: rest, options, allowPositionals: true }));
  } catch (e) {
    if (command === "setup") throw new Error(`${e.message}\n${setupUsage(setupAdapter)}`, { cause: e });
    throw e;
  }
  const [min, max] = spec.positional;
  if (positionals.length < min || positionals.length > max) throw new Error(USAGE);
  if (command === "whoami") {
    if (values.select?.length !== 1 || !values.select[0].trim()) {
      throw new Error("whoami needs exactly one nonempty --select CREDENTIAL");
    }
    for (const flag of Object.keys(values)) {
      if (!["select", "json"].includes(flag)) throw new Error(`whoami does not support --${flag}`);
    }
  }
  return {
    command,
    opts: {
      ...values,
      positional: positionals,
      select: values.select ?? [],
      limit: values.limit === undefined ? undefined : Number(values.limit),
      concurrency: values.concurrency === undefined ? 1 : Number(values.concurrency),
    },
  };
}

// Runs one command against the store and returns the exit code. `io` receives the output so the
// CLI tests can capture it.
export async function main(argv, io = console) {
  let parsed;
  try {
    if (argv[0] === "setup") {
      // Only discovery is permissive: strict parsing with the selected app's schema must finish
      // before opening the store or calling a hook that may launch a browser.
      adapters = await loadAdapters();
      APP_ADAPTERS = adapters.map(a => a.name);
      const { positionals } = parseArgs({ args: argv.slice(1), options: OPTIONS, allowPositionals: true, strict: false });
      if (!positionals.length) throw new Error(USAGE);
      parsed = parseCli(argv, getAdapter(positionals[0]));
    } else parsed = parseCli(argv);
  } catch (e) {
    io.error(e.message);
    return 1;
  }
  let db;
  try {
    if (parsed.command !== "setup") adapters = await loadAdapters(parsed.command === "validate" ? { paths: parsed.opts.positional.map(p => resolve(p)) } : {});
    APP_ADAPTERS = adapters.map(a => a.name);
    SESSION_TABLES = [...APP_ADAPTERS, ...Object.keys(USERNAME_TABLES)];
    if (parsed.command === "setup" && parsed.opts.help) { io.log(setupUsage(getAdapter(parsed.opts.positional[0]))); return 0; }
    if (parsed.command === "validate") { io.log(APP_ADAPTERS.join(", ")); return 0; }
    if (parsed.command === "whoami") return await runWhoami(parsed.opts, io);
    db = store.openDb(config.dbPath());
    return await COMMANDS[parsed.command].run(db, parsed.opts, io);
  } catch (e) {
    const message = parsed.command === "whoami" ? e.message.replaceAll(parsed.opts.select[0], "[redacted]") : e.message;
    io.error(message);
    return 1;
  } finally {
    db?.close();
  }
}

// Node resolves the entry module to its real path, so a symlinked invocation must be resolved too.
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  main(process.argv.slice(2)).then((code) => {
    process.exitCode = code;
  });
}
