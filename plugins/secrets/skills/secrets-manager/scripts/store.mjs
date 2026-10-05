// ABOUTME: SQLite store for Google accounts, each app's exported login state, and the X and TikTok logins.
// ABOUTME: Google accounts live in one table; every app gets its own table, never shared columns.

import { readFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";

// One status vocabulary for every table (google identities, app sessions, x logins):
//   new        imported, no login run has recorded an outcome yet
//   active     usable
//   expired    session/token no longer valid, re-login needed
//   restricted banned or blocked by the app
//   escalated  needs a human to resolve
export const STATUS_NEW = "new";
export const STATUS_ACTIVE = "active";
export const STATUS_EXPIRED = "expired";
export const STATUS_RESTRICTED = "restricted";
export const STATUS_ESCALATED = "escalated";
export const STATUSES = new Set([STATUS_NEW, STATUS_ACTIVE, STATUS_EXPIRED, STATUS_RESTRICTED, STATUS_ESCALATED]);

const STATUS_CHECK = "CHECK (status IN ('new','active','expired','restricted','escalated'))";

// Tables that are not app-session tables, excluded when listing apps.
const NON_APP_TABLES = new Set(["google", "x", "tiktok"]);

const SCHEMA_PATH = fileURLToPath(new URL("./schema.sql", import.meta.url));

export function now() {
  return new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
}

function assertStatus(status) {
  if (!STATUSES.has(status)) throw new Error(`unknown status: ${JSON.stringify(status)}`);
}

// Normalise an app name into a safe SQLite identifier (also the table name).
export function toAppSlug(app) {
  const slug = app.trim().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "");
  if (!slug) throw new Error(`app name has no usable characters: ${JSON.stringify(app)}`);
  return slug;
}

// Open the canonical store, creating the fixed tables if absent. App session tables are created
// on demand by saveSession.
export function openDb(path) {
  if (path !== ":memory:") mkdirSync(dirname(path), { recursive: true });
  const db = new DatabaseSync(path);
  if (path !== ":memory:") {
    db.exec("PRAGMA journal_mode=WAL");
    // Wait out a transient lock instead of throwing SQLITE_BUSY. Within one process node:sqlite is
    // synchronous so parallel login workers can never write at the same instant, but a second process
    // sharing the file (or a WAL checkpoint) can still collide; the timeout makes the writer retry.
    db.exec("PRAGMA busy_timeout=5000");
  }
  db.exec(readFileSync(SCHEMA_PATH, "utf8"));
  migrateGoogleStatus(db);
  return db;
}

// The `status` CHECK originally omitted 'new'. CREATE TABLE IF NOT EXISTS never alters an existing
// table, so a store created before 'new' still rejects it. Widen the live google table's CHECK by
// rebuilding it inside a transaction, preserving every row. A fresh database already carries the
// widened CHECK from schema.sql, so the guard makes this a no-op there.
function migrateGoogleStatus(db) {
  const row = db.prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='google'").get();
  if (!row || row.sql.includes("'new'")) return;
  db.exec("BEGIN");
  try {
    db.exec(`CREATE TABLE google_migrate (
      email          TEXT PRIMARY KEY,
      password       TEXT NOT NULL,
      totp_secret    TEXT,
      app_password   TEXT,
      status         TEXT NOT NULL DEFAULT 'new' ${STATUS_CHECK},
      profile_dir    TEXT,
      proxy          TEXT,
      last_login_at  TEXT,
      created_at     TEXT NOT NULL,
      updated_at     TEXT NOT NULL
    )`);
    db.exec(
      `INSERT INTO google_migrate (email, password, totp_secret, app_password, status, profile_dir, proxy, last_login_at, created_at, updated_at)
       SELECT email, password, totp_secret, app_password, status, profile_dir, proxy, last_login_at, created_at, updated_at FROM google`,
    );
    db.exec("DROP TABLE google");
    db.exec("ALTER TABLE google_migrate RENAME TO google");
    db.exec("COMMIT");
  } catch (e) {
    db.exec("ROLLBACK");
    throw e;
  }
}

// --- Google accounts -------------------------------------------------------

// Insert a new account or refresh a known one's credentials. True if newly added. An existing
// account keeps its status and last_login_at; only the credentials read from the file are
// refreshed. (profile_dir and proxy are legacy columns: the profile is derived from the email and
// the proxy from the environment, so neither is written.)
export function upsertAccount(db, email, password, totpSecret, appPassword = null) {
  const ts = now();
  if (!getAccount(db, email)) {
    db.prepare(
      `INSERT INTO google (email, password, totp_secret, app_password, status, last_login_at, created_at, updated_at)
       VALUES (?, ?, ?, ?, ?, NULL, ?, ?)`,
    ).run(email, password, totpSecret, appPassword, STATUS_NEW, ts, ts);
    return true;
  }
  db.prepare("UPDATE google SET password = ?, totp_secret = ?, app_password = ?, updated_at = ? WHERE email = ?").run(
    password,
    totpSecret,
    appPassword,
    ts,
    email,
  );
  return false;
}

export function getAccount(db, email) {
  return db.prepare("SELECT * FROM google WHERE email = ?").get(email) ?? null;
}

export function listAccounts(db) {
  return db.prepare("SELECT * FROM google ORDER BY email").all();
}

export function setAccountStatus(db, email, status) {
  assertStatus(status);
  db.prepare("UPDATE google SET status = ?, updated_at = ? WHERE email = ?").run(status, now(), email);
}

export function markLoggedIn(db, email) {
  const ts = now();
  // A confirmed Google sign-in clears any prior escalation/expiry — the challenge that raised it is gone.
  db.prepare("UPDATE google SET status = ?, last_login_at = ?, updated_at = ? WHERE email = ?").run(STATUS_ACTIVE, ts, ts, email);
}

// --- Per-app sessions ------------------------------------------------------

// Create this app's own session table if absent; return the table name.
export function ensureAppTable(db, app) {
  const table = toAppSlug(app);
  db.exec(
    `CREATE TABLE IF NOT EXISTS "${table}" (
       email TEXT PRIMARY KEY,
       cookies TEXT NOT NULL,
       local_storage TEXT NOT NULL,
       status TEXT NOT NULL DEFAULT 'active' ${STATUS_CHECK},
       created_at TEXT NOT NULL,
       updated_at TEXT NOT NULL
     )`,
  );
  return table;
}

// Whether a table carries the app-session shape (keyed by an email column).
function hasEmailColumn(db, table) {
  return db.prepare(`PRAGMA table_info("${table}")`).all().some((c) => c.name === "email");
}

// Every app-session table present in the store, alphabetically. A stray table
// (e.g. left by a mistyped sqlite command) without an email column is ignored,
// so it never breaks the per-account session lookup.
export function listApps(db) {
  return db
    .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    .all()
    .map((r) => r.name)
    .filter((name) => !NON_APP_TABLES.has(name) && !name.startsWith("sqlite_"))
    .filter((name) => hasEmailColumn(db, name));
}

// Store a freshly exported (account, app) login state, marking it active.
export function saveSession(db, app, email, cookies, localStorage) {
  const table = ensureAppTable(db, app);
  const ts = now();
  db.prepare(
    `INSERT INTO "${table}" (email, cookies, local_storage, status, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?)
     ON CONFLICT(email) DO UPDATE SET
       cookies = excluded.cookies,
       local_storage = excluded.local_storage,
       status = excluded.status,
       updated_at = excluded.updated_at`,
  ).run(email, JSON.stringify(cookies), JSON.stringify(localStorage), STATUS_ACTIVE, ts, ts);
}

export function getSession(db, app, email) {
  const table = ensureAppTable(db, app);
  const row = db.prepare(`SELECT * FROM "${table}" WHERE email = ?`).get(email);
  if (!row) return null;
  return {
    email: row.email,
    app,
    status: row.status,
    updated_at: row.updated_at,
    cookies: JSON.parse(row.cookies),
    local_storage: JSON.parse(row.local_storage),
  };
}

export function setSessionStatus(db, app, email, status) {
  assertStatus(status);
  const table = ensureAppTable(db, app);
  const { changes } = db
    .prepare(`UPDATE "${table}" SET status = ?, updated_at = ? WHERE email = ?`)
    .run(status, now(), email);
  if (changes === 0) throw new Error(`${email} has no ${app} session`);
}

// Record an app's verdict on an account, creating its row when the app never gave it a session (an
// app that bans an account at sign-in leaves no cookies to store). A stored session keeps its state.
export function recordSessionStatus(db, app, email, status) {
  assertStatus(status);
  const table = ensureAppTable(db, app);
  const ts = now();
  db.prepare(
    `INSERT INTO "${table}" (email, cookies, local_storage, status, created_at, updated_at)
     VALUES (?, '[]', '[]', ?, ?, ?)
     ON CONFLICT(email) DO UPDATE SET status = excluded.status, updated_at = excluded.updated_at`,
  ).run(email, status, ts, ts);
}

// Map each app that has a session for this account to that session's status.
export function sessionsForAccount(db, email) {
  const out = {};
  for (const app of listApps(db)) {
    const session = getSession(db, app, email);
    if (session) out[app] = session.status;
  }
  return out;
}

// --- X logins ----------------------------------------------------------------

// Insert an imported X account, or refresh the credentials of a known one. A ct0 only works with
// the auth_token of its own session, so an import that brings another auth_token drops the stored
// ct0 and cookies; the row is then one `verify x` (or `login x`) picks up.
export function upsertX(db, account) {
  const ts = now();
  const tokenChanged = "NULLIF(excluded.auth_token, '') IS NOT NULL AND excluded.auth_token IS NOT x.auth_token";
  db.prepare(
    `INSERT INTO x (username, password, email, email_password, totp_secret, auth_token, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(username) DO UPDATE SET
       password = excluded.password,
       email = COALESCE(NULLIF(excluded.email, ''), x.email),
       email_password = COALESCE(NULLIF(excluded.email_password, ''), x.email_password),
       totp_secret = COALESCE(NULLIF(excluded.totp_secret, ''), x.totp_secret),
       auth_token = COALESCE(NULLIF(excluded.auth_token, ''), x.auth_token),
       ct0 = CASE WHEN ${tokenChanged} THEN NULL ELSE x.ct0 END,
       cookies = CASE WHEN ${tokenChanged} THEN NULL ELSE x.cookies END,
       updated_at = excluded.updated_at`,
  ).run(
    account.username,
    account.password,
    account.email ?? null,
    account.email_password ?? null,
    account.totp_secret ?? null,
    account.auth_token ?? null,
    ts,
    ts,
  );
}

// X rows to log in: those without a usable ct0, or all with force, or one named user (an explicit
// target, e.g. relogin after a 401, attempted regardless of state).
export function getPendingX(db, { force, user, limit } = {}) {
  let sql;
  const params = [];
  if (user) {
    sql = "SELECT * FROM x WHERE username = ? ORDER BY username";
    params.push(user);
  } else if (force) {
    sql = "SELECT * FROM x ORDER BY username";
  } else {
    sql = "SELECT * FROM x WHERE ct0 IS NULL ORDER BY username";
  }
  if (limit) {
    sql += " LIMIT ?";
    params.push(limit);
  }
  return db.prepare(sql).all(...params);
}

// Accounts that carry a stored auth_token to turn into a usable pair (deriving ct0).
export function getVerifiable(db, { force, user, limit } = {}) {
  let sql = "SELECT * FROM x WHERE auth_token IS NOT NULL";
  const params = [];
  if (user) {
    sql += " AND username = ?";
    params.push(user);
  } else if (!force) {
    sql += " AND ct0 IS NULL";
  }
  sql += " ORDER BY username";
  if (limit) {
    sql += " LIMIT ?";
    params.push(limit);
  }
  return db.prepare(sql).all(...params);
}

export function saveXLogin(db, username, { authToken, ct0, cookies }) {
  const { changes } = db
    .prepare(
      `UPDATE x SET auth_token = ?, ct0 = ?, cookies = ?, status = 'active', updated_at = ?
       WHERE username = ?`,
    )
    .run(authToken, ct0, JSON.stringify(cookies), now(), username);
  if (changes === 0) throw new Error(`no X account ${JSON.stringify(username)}`);
}

// A login/verify failure carries no free-text column (by design): the reason is logged to the
// console and the row's usability is recorded as a status. `expired` = retry later, `escalated` =
// needs a human (a non-TOTP challenge).
export function setXStatus(db, username, status) {
  setLoginStatus(db, "x", "X", username, status);
}

// Set the status of one row of a username-keyed login table (`x`, `tiktok`).
function setLoginStatus(db, table, label, username, status) {
  assertStatus(status);
  const { changes } = db
    .prepare(`UPDATE ${table} SET status = ?, updated_at = ? WHERE username = ?`)
    .run(status, now(), username);
  if (changes === 0) throw new Error(`no ${label} account ${JSON.stringify(username)}`);
}

// --- TikTok logins -------------------------------------------------------------

// Insert a TikTok account or refresh a known one's credentials; a known account keeps its
// session, slot and status.
export function upsertTiktok(db, account) {
  const ts = now();
  db.prepare(
    `INSERT INTO tiktok (username, password, email, email_password, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?)
     ON CONFLICT(username) DO UPDATE SET
       password = excluded.password,
       email = COALESCE(NULLIF(excluded.email, ''), tiktok.email),
       email_password = COALESCE(NULLIF(excluded.email_password, ''), tiktok.email_password),
       updated_at = excluded.updated_at`,
  ).run(account.username, account.password, account.email ?? null, account.email_password ?? null, ts, ts);
}

export function getTiktok(db, username) {
  return db.prepare("SELECT * FROM tiktok WHERE username = ?").get(username) ?? null;
}

// TikTok rows to log in: those with no stored session or an expired one, or all with force.
export function getPendingTiktok(db, { force } = {}) {
  const where = force ? "" : "WHERE cookies IS NULL OR status = 'expired'";
  return db.prepare(`SELECT * FROM tiktok ${where} ORDER BY username`).all();
}

// Store a fresh TikTok session and the ISP slot it was created on, marking the account active.
export function saveTiktokLogin(db, username, { cookies, ispSlot }) {
  const { changes } = db
    .prepare("UPDATE tiktok SET cookies = ?, isp_slot = ?, status = 'active', updated_at = ? WHERE username = ?")
    .run(JSON.stringify(cookies), ispSlot, now(), username);
  if (changes === 0) throw new Error(`no TikTok account ${JSON.stringify(username)}`);
}

export function setTiktokStatus(db, username, status) {
  setLoginStatus(db, "tiktok", "TikTok", username, status);
}
