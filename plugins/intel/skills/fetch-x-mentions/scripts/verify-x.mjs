// Verify vendor X tokens into ct0 pairs in the existing secrets store.
import { realpathSync } from "node:fs";
import { DatabaseSync } from 'node:sqlite';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { requireEnv, loadEnvFile } from './env.mjs';
import { provisionPair } from './fetch-x-mentions.mjs';

export async function verifyX(db, opts, { provision = provisionPair, dispatcher, io = console } = {}) {
  const concurrency = Number(opts.concurrency ?? 1);
  if (!Number.isInteger(concurrency) || concurrency < 1) throw new Error('--concurrency must be a positive integer');
  const rows = db.prepare('SELECT * FROM x WHERE auth_token IS NOT NULL ORDER BY username').all()
    .filter(row => opts.select?.length ? opts.select.includes(row.username) : opts.all || row.ct0 === null);
  if (!rows.length) { io.log('no rows with a stored token to check'); return 0; }
  let cursor = 0, failed = false;
  await Promise.all(Array.from({length: Math.min(concurrency, rows.length)}, async () => {
    while (cursor < rows.length) {
      const account = rows[cursor++];
      try {
        const { ok, ct0 } = await provision(account.auth_token, dispatcher);
        if (ok && ct0) {
          db.prepare("UPDATE x SET ct0=?, cookies=?, status='active', updated_at=? WHERE username=?")
            .run(ct0, JSON.stringify({auth_token:account.auth_token,ct0}), new Date().toISOString(), account.username);
          io.log(`${account.username}: ok, pair stored`);
        } else {
          db.prepare("UPDATE x SET status='expired', updated_at=? WHERE username=?").run(new Date().toISOString(),account.username);
          io.log(`${account.username}: not usable`);
        }
      } catch (e) {
        failed = true;
        db.prepare("UPDATE x SET status='expired', updated_at=? WHERE username=?").run(new Date().toISOString(),account.username);
        io.error(`${account.username}: ${e.message}`);
      }
    }
  }));
  return failed ? 1 : 0;
}

export async function main(argv) {
  let db, dispatcher;
  try {
    const {values,positionals}=parseArgs({args:argv,options:{select:{type:'string',multiple:true},all:{type:'boolean'},concurrency:{type:'string'}},allowPositionals:true});
    if (positionals.length) throw new Error('Usage: verify-x.mjs [--select USER]... [--all] [--concurrency N]');
    loadEnvFile();
    const url=requireEnv('RESIDENTIAL_PROXY_URL'), u=new URL(url);
    const {ProxyAgent}=await import('undici');
    const token=u.username ? `Basic ${Buffer.from(`${decodeURIComponent(u.username)}:${decodeURIComponent(u.password)}`).toString('base64')}` : null;
    dispatcher=new ProxyAgent(token ? {uri:`${u.protocol}//${u.host}`,token} : `${u.protocol}//${u.host}`);
    const state=process.env.SECRETS_DATA_DIR || join(homedir(),'.config/secrets-manager');
    db=new DatabaseSync(join(state,'secrets.sqlite'));
    return await verifyX(db,values,{dispatcher});
  } catch (e) { console.error(e.message); return 1; }
  finally { db?.close(); await dispatcher?.close(); }
}
if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode=await main(process.argv.slice(2));
