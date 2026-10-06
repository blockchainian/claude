-- ABOUTME: The secrets-manager skill's fixed tables in the store (~/.config/secrets-manager/secrets.sqlite).
-- ABOUTME: `google` (accounts), `x` (X logins) and `tiktok` (TikTok logins); per-app session tables  are created on demand by store.mjs.
--
-- Applied idempotently on every open (CREATE TABLE IF NOT EXISTS). Column names are snake_case.
-- Credentials are plaintext; the file lives outside the repo. Status vocabulary:
-- new | active | expired | restricted | escalated; app sessions also support ready (setup done).
-- A google account is `new` on import and only leaves `new` when a login run records an outcome
-- (active on success, or expired/restricted/escalated).

-- Google accounts: credentials + status per account. profile_dir is the persistent Camoufox
-- profile (the "device" Google sees); proxy is the residential proxy the account logs in through.
CREATE TABLE IF NOT EXISTS google (
  email          TEXT PRIMARY KEY,
  password       TEXT NOT NULL,
  totp_secret    TEXT,
  app_password   TEXT,
  status         TEXT NOT NULL DEFAULT 'new'
                   CHECK (status IN ('new','active','ready','expired','restricted','escalated')),
  profile_dir    TEXT,
  proxy          TEXT,
  last_login_at  TEXT,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);

-- X / Twitter: own credentials + login tokens. auth_token and ct0 are mirrored out of the cookie
-- jar for direct use by the fetch-x-mentions scraper; `cookies` holds the full name->value jar as JSON.
CREATE TABLE IF NOT EXISTS x (
  email          TEXT PRIMARY KEY NOT NULL,
  username       TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password       TEXT NOT NULL,
  email_password TEXT,
  totp_secret    TEXT,
  auth_token     TEXT,
  ct0            TEXT,
  cookies        TEXT,
  status         TEXT NOT NULL DEFAULT 'active'
                   CHECK (status IN ('new','active','expired','restricted','escalated')),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);

-- TikTok: own credentials + the logged-in session. `cookies` holds the browser's tiktok.com
-- cookies as a JSON array; isp_slot is the ISP pool slot (a fixed IP) the session was created on,
-- the only exit it is used from.
CREATE TABLE IF NOT EXISTS tiktok (
  username       TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
  password       TEXT NOT NULL,
  email          TEXT,
  email_password TEXT,
  cookies        TEXT,
  isp_slot       INTEGER,
  status         TEXT NOT NULL DEFAULT 'new'
                   CHECK (status IN ('new','active','expired','restricted','escalated')),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);
