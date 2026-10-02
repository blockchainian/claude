// ABOUTME: Reads a signup one-time code from a Gmail inbox over IMAP, and extracts the digits.
// ABOUTME: extractOtp/otpCandidates are pure and tested; readSignupOtp talks to real Gmail.

import { ImapFlow } from "imapflow";
import { simpleParser } from "mailparser";

// The email signup code is a 6-digit run; a keyword ("code"/"verification") near it breaks ties
// against a stray year or footer number.
const OTP_RE = /\b\d{6}\b/g;
const KEYWORD_RE = /\b(code|verification|verify|otp|pin|passcode)\b/gi;

// Every 6-digit run in the text (the OTP length), in order of appearance.
export function otpCandidates(text) {
  return [...(text || "").matchAll(OTP_RE)].map((m) => m[0]);
}

// The verification code from an OTP email body: the 6-digit run nearest a code keyword. Falls
// back to the first 6-digit run when no keyword is present. Returns null when the text has no
// 6-digit run (a longer digit sequence like an id is not an OTP).
export function extractOtp(text) {
  if (!text) return null;
  const runs = [...text.matchAll(OTP_RE)].map((m) => ({ at: m.index, code: m[0] }));
  if (runs.length === 0) return null;
  const keywords = [...text.matchAll(KEYWORD_RE)].map((m) => m.index);
  if (keywords.length) {
    const distance = (r) => Math.min(...keywords.map((k) => Math.abs(r.at - k)));
    runs.sort((a, b) => distance(a) - distance(b));
  }
  return runs[0].code;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Best-effort plain text of a parsed email, preferring text/plain over text/html.
function messageText(parsed) {
  if (parsed.text) return parsed.text;
  return (parsed.html || "").replace(/<[^>]+>/g, " ");
}

// Poll a Gmail inbox over IMAP for the newest verification email and return its code.
// - `appPassword` is the 16-char Google app password (spaces are stripped).
// - `toAlias`, when set, keeps only messages whose `To`/`Delivered-To` carries that plus-alias,
//   so several signups sharing one inbox don't cross their codes.
// - `sinceEpoch` (seconds) drops anything received before the click (IMAP SINCE is date-only).
// Returns {otp, from, subject, to, folder} for the matched message, {securityAlert: true, otp:
// null, ...} when the address got a "Security Alert" mail instead (already registered), or null
// on timeout. Checks the Spam folder once after the inbox times out.
export async function readSignupOtp(baseEmail, appPassword, { toAlias = null, sinceEpoch = null, timeoutS = 120, pollS = 5 } = {}) {
  const client = new ImapFlow({
    host: "imap.gmail.com",
    port: 993,
    secure: true,
    auth: { user: baseEmail, pass: (appPassword || "").replace(/ /g, "") },
    logger: false,
  });
  const deadline = Date.now() + timeoutS * 1000;
  await client.connect();
  try {
    let checkedSpam = false;
    for (;;) {
      const hit = await scanFolder(client, "INBOX", toAlias, sinceEpoch);
      if (hit) return hit;
      if (Date.now() >= deadline) {
        if (!checkedSpam) {
          checkedSpam = true;
          const spam = await scanFolder(client, "[Gmail]/Spam", toAlias, sinceEpoch);
          if (spam) return spam;
        }
        return null;
      }
      await sleep(pollS * 1000);
    }
  } finally {
    await client.logout().catch(() => {});
  }
}

// Search one folder for the newest verification email carrying a 6-digit code.
async function scanFolder(client, folder, toAlias, sinceEpoch) {
  let lock;
  try {
    lock = await client.getMailboxLock(folder, { readOnly: true });
  } catch {
    return null;
  }
  try {
    // IMAP SINCE is date-only and Gmail evaluates it in the mailbox's own timezone, so a UTC
    // date can exclude mail that arrived minutes ago; ask from the previous day and filter by
    // the parsed Date header below.
    const since = new Date(((sinceEpoch ?? Date.now() / 1000) - 24 * 3600) * 1000);
    since.setUTCHours(0, 0, 0, 0);
    const seqs = await client.search({ since });
    if (!seqs || seqs.length === 0) return null;
    for (const seq of [...seqs].reverse()) {
      const msg = await client.fetchOne(seq, { source: true });
      if (!msg?.source) continue;
      const parsed = await simpleParser(msg.source);
      if (sinceEpoch !== null && parsed.date) {
        if (parsed.date.getTime() / 1000 < sinceEpoch - 90) continue; // clock/relay skew
      }
      const delivered = parsed.headers.get("delivered-to");
      const toLine = [parsed.to?.text, typeof delivered === "string" ? delivered : delivered?.text]
        .filter(Boolean)
        .join(" ");
      if (toAlias && !toLine.toLowerCase().includes(toAlias.toLowerCase())) continue;
      const base = { from: parsed.from?.text ?? "", subject: parsed.subject ?? "", to: toLine, folder };
      // A signup on an already-registered address gets a "Security Alert" mail and no code.
      if (/security alert/i.test(base.subject)) return { ...base, otp: null, securityAlert: true };
      const otp = extractOtp(messageText(parsed));
      if (!otp) continue;
      return { ...base, otp };
    }
    return null;
  } finally {
    lock.release();
  }
}
