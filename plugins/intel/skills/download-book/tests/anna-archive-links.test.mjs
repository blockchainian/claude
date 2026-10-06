// ABOUTME: Tests slow-download entry selection: the fast waitlist server is chosen over the throttled no-waitlist one.
// ABOUTME: Uses a small inline fixture mirroring the site's per-<li> option list; no network.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, symlinkSync, writeFileSync, rmSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseLinks, selectSlowPaths, readMemberKey, leftEntry, orderSlowPaths, downloadFirst } from '../scripts/anna-archive-links.mjs';

const MD5 = '26f03228f2f3ee0f980ae56f9bd97844';
const fixture = `<ul class="list-inside mb-4 ml-1">
  <li class="list-disc"><a href="/slow_download/${MD5}/0/0" class="js-download-link">Slow Partner Server #1</a> (slightly faster but with waitlist)</li>
  <li class="list-disc"><a href="/slow_download/${MD5}/0/1" class="js-download-link">Slow Partner Server #2</a> (slightly faster but with waitlist)</li>
  <li class="list-disc"><a href="/slow_download/${MD5}/0/8" class="js-download-link">Slow Partner Server #9</a> (no waitlist, but can be very slow)</li>
  <li class="list-disc"><a href="/slow_download/${MD5}/0/9" class="js-download-link">Slow Partner Server #10</a> (no waitlist, but can be very slow)</li>
</ul>`;

test('selectSlowPaths prefers the first waitlist server over the no-waitlist ones', () => {
  const anchors = parseLinks(fixture).anchors;
  const { primary, fallback } = selectSlowPaths(anchors, MD5);
  assert.ok(primary.href.endsWith('/0/0'), `primary should be the first waitlist server, got ${primary?.href}`);
  assert.ok(fallback?.href.endsWith('/0/8'), `fallback should be the first no-waitlist server, got ${fallback?.href}`);
});

test('selectSlowPaths falls back to the first entry when no waitlist labels exist', () => {
  const only = `<ul><li class="list-disc"><a href="/slow_download/${MD5}/0/8" class="js-download-link">Server</a> (no waitlist, but can be very slow)</li></ul>`;
  const { primary, fallback } = selectSlowPaths(parseLinks(only).anchors, MD5);
  assert.ok(primary.href.endsWith('/0/8'), 'primary falls back to the only entry');
  assert.equal(fallback, null, 'no separate fallback when the only entry is already primary');
});

test('orderSlowPaths lists the waitlist servers first, then the no-waitlist ones, in page order', () => {
  const order = orderSlowPaths(parseLinks(fixture).anchors, MD5).map(anchor => anchor.href.split('/').pop());
  assert.deepEqual(order, ['0', '1', '8', '9']);
});

const EPUB = Buffer.from('PK\x03\x04mimetypeapplication/epub+zip');
const serve = statuses => {
  const asked = [];
  const fetchFile = async url => {
    asked.push(url);
    const status = statuses[url];
    return new Response(status === 200 ? EPUB : 'Too many downloads at the same time from the same IP address', { status });
  };
  return { asked, fetchFile };
};

test('downloadFirst moves on to the next link when one answers 429, resolving it only then', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'anna-dl-'));
  const out = join(dir, 'book.epub');
  const { asked, fetchFile } = serve({ 'https://a/1': 429, 'https://b/2': 200 });
  let resolvedThird = false;
  const result = await downloadFirst([() => 'https://a/1', async () => 'https://b/2', () => { resolvedThird = true; return 'https://c/3'; }], out, fetchFile);
  assert.deepEqual(asked, ['https://a/1', 'https://b/2']);
  assert.equal(resolvedThird, false, 'a link after the one that worked is never resolved');
  assert.deepEqual(result, { path: out, url: 'https://b/2', refused: ['https://a/1'] });
  assert.deepEqual(readFileSync(out), EPUB);
  rmSync(dir, { recursive: true });
});

test('downloadFirst skips a resolver with no link and fails when every link answers 429', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'anna-dl-'));
  const out = join(dir, 'book.epub');
  const { fetchFile } = serve({ 'https://a/1': 429, 'https://b/2': 429 });
  await assert.rejects(downloadFirst([() => 'https://a/1', () => null, () => 'https://b/2'], out, fetchFile), /2 个链接都返回 429/);
  assert.equal(existsSync(out), false);
  rmSync(dir, { recursive: true });
});

test('downloadFirst stops on a failure other than 429 and refuses a file that is not an EPUB', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'anna-dl-'));
  const out = join(dir, 'book.epub');
  const { asked, fetchFile } = serve({ 'https://a/1': 404, 'https://b/2': 200 });
  await assert.rejects(downloadFirst([() => 'https://a/1', () => 'https://b/2'], out, fetchFile), /HTTP 404/);
  assert.deepEqual(asked, ['https://a/1']);
  const html = async () => new Response('<html>error</html>', { status: 200 });
  await assert.rejects(downloadFirst([() => 'https://a/1'], out, html), /不是 EPUB/);
  assert.equal(existsSync(out), false);
  rmSync(dir, { recursive: true });
});

test('leftEntry accepts the slow entry redirecting to itself with a bare query', () => {
  const entry = `https://annas-archive.pk/slow_download/${MD5}/0/0`;
  assert.equal(leftEntry(entry, `${entry}?`), false);
  assert.equal(leftEntry(entry, entry), false);
});

test('leftEntry rejects a redirect to another path or origin', () => {
  const entry = `https://annas-archive.pk/slow_download/${MD5}/0/0`;
  assert.equal(leftEntry(entry, 'https://annas-archive.pk/account/'), true);
  assert.equal(leftEntry(entry, `https://example.com/slow_download/${MD5}/0/0`), true);
});

test('the script runs when invoked through a symlinked directory', () => {
  const dir = mkdtempSync(join(tmpdir(), 'anna-link-'));
  const scripts = fileURLToPath(new URL('../scripts/', import.meta.url));
  symlinkSync(scripts, join(dir, 'scripts'));
  const run = spawnSync(process.execPath, [join(dir, 'scripts', 'anna-archive-links.mjs'), '--help'], { encoding: 'utf8' });
  assert.match(run.stdout + run.stderr, /用法/, 'a symlinked invocation must print the usage line, not exit silently');
});

test('the member key comes from the dotenv file, ignoring process environment', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'anna-config-'));
  const configPath = join(dir, '.env');
  const previous = process.env.ANNA_ARCHIVE_SECRET_KEY;
  process.env.ANNA_ARCHIVE_SECRET_KEY = 'ignored-environment-value';
  try {
    writeFileSync(configPath, 'ANNA_ARCHIVE_SECRET_KEY="file-key#literal" # comment\n');
    assert.equal(await readMemberKey(configPath), 'file-key#literal');
    writeFileSync(configPath, 'ANNA_ARCHIVE_SECRET_KEY=""\n');
    await assert.rejects(readMemberKey(configPath), /配置 ANNA_ARCHIVE_SECRET_KEY/);
    writeFileSync(configPath, 'OTHER_KEY=value\n');
    await assert.rejects(readMemberKey(configPath), /配置 ANNA_ARCHIVE_SECRET_KEY/);
    await assert.rejects(readMemberKey(join(dir, 'missing')), { code: 'ENOENT' });
  } finally {
    if (previous === undefined) delete process.env.ANNA_ARCHIVE_SECRET_KEY;
    else process.env.ANNA_ARCHIVE_SECRET_KEY = previous;
    rmSync(dir, { recursive: true });
  }
});
