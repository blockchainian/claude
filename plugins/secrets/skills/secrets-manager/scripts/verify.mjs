// ABOUTME: Builtin account checks behind `verify google|x|tiktok`: which rows each target holds and
// ABOUTME: how a row's status is written back. `verify` tries these before any adapter's verify hook.
import * as store from "./store.mjs";
import { checkX } from "./x-verify.mjs";

// The statuses a check may return. `escalated` = usable only after a person clears something (X lock).
export const CHECK_RESULTS = [store.STATUS_ACTIVE, store.STATUS_EXPIRED, store.STATUS_RESTRICTED, store.STATUS_ESCALATED];

// The status to store after a check. `expired` only says the stored token no longer works, which a
// banned (`restricted`) or person-blocked (`escalated`) account's token usually also is, so it never
// overwrites those; any other result is newer knowledge and is written.
export function nextStatus(current, result) {
  const kept = result === store.STATUS_EXPIRED && (current === store.STATUS_RESTRICTED || current === store.STATUS_ESCALATED);
  return kept ? current : result;
}

// Each entry: `label` for messages, `rows(db)` every account of the target, `id(row)` its key,
// `setStatus(db, id, status)`, and `check({db, row, opts, io})` returning one of CHECK_RESULTS or
// throwing when the probe cannot tell (the status is then left as it was).
export const BUILTIN_CHECKS = {
  google: {
    label: "Google",
    // A plus-alias row (base+tag@) is an app's own alias account, signed in by email, not Google.
    rows: (db) => store.listAccounts(db).filter((row) => !row.email.split("@")[0].includes("+")),
    id: (row) => row.email,
    setStatus: store.setAccountStatus,
    check: async (args) => (await import("./login.mjs")).checkGoogle(args),
  },
  x: {
    label: "X",
    rows: (db) => store.getPendingX(db, { force: true }),
    id: (row) => row.username,
    setStatus: store.setXStatus,
    check: checkX,
  },
  tiktok: {
    label: "TikTok",
    rows: (db) => store.getPendingTiktok(db, { force: true }),
    id: (row) => row.username,
    setStatus: store.setTiktokStatus,
    check: async (args) => (await import("./tiktok-login.mjs")).checkTiktok(args),
  },
};
