// ABOUTME: Tests the session wrapper's challenge handling against a fake page, and how runs share one Chrome — no network:
// ABOUTME: a run attaches to the Chrome that is up or starts it once; the Chrome is kept until no run has had a tab for a while.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { attach, sessionFor, isChallenge, untilIdle } from '../scripts/site-session.mjs';

const CHALLENGE_PAGE = '<!doctype html><html><head><title>DDoS-Guard</title></head><body>Checking your browser</body></html>';

function fakePage(bodies) {
  const calls = { evaluate: [], goto: [], waits: 0 };
  return {
    calls,
    async evaluate(_fn, [url, follow]) { calls.evaluate.push([url, follow]); return { status: 200, redirected: false, body: bodies.shift() }; },
    async goto(url) { calls.goto.push(url); },
    async waitForFunction() { calls.waits++; },
  };
}

test('isChallenge recognizes the challenge and captcha pages, not a site page mentioning DDoS-Guard', () => {
  assert.equal(isChallenge(CHALLENGE_PAGE), true);
  assert.equal(isChallenge('<html><head><title>DDOS-GUARD</title><link href="/.well-known/ddos-guard/ddg-captcha-page/index.css"></head></html>'), true);
  assert.equal(isChallenge('<html><div class="js-aarecord-list-outer"></div><script>// "text/css" for DDOS-GUARD caching.</script></html>'), false);
});

test('a normal body is returned without navigating', async () => {
  const page = fakePage(['<html>results</html>']);
  const result = await sessionFor(page).get('https://example.test/search', true);
  assert.equal(result.body, '<html>results</html>');
  assert.deepEqual(page.calls.goto, []);
  assert.deepEqual(page.calls.evaluate, [['https://example.test/search', true]]);
});

test('a challenge body navigates the page there, waits, then refetches once', async () => {
  const page = fakePage([CHALLENGE_PAGE, '<html>results</html>']);
  const result = await sessionFor(page).get('https://example.test/md5/abc', false);
  assert.equal(result.body, '<html>results</html>');
  assert.deepEqual(page.calls.goto, ['https://example.test/md5/abc']);
  assert.equal(page.calls.waits, 1);
  assert.deepEqual(page.calls.evaluate, [['https://example.test/md5/abc', false], ['https://example.test/md5/abc', false]]);
});

test('a challenge that survives the navigation is returned as is', async () => {
  const page = fakePage([CHALLENGE_PAGE, CHALLENGE_PAGE]);
  const result = await sessionFor(page).get('https://example.test/x');
  assert.equal(isChallenge(result.body), true);
  assert.equal(page.calls.goto.length, 1);
});

test('a body over 3 MB is rejected', async () => {
  const page = fakePage(['x'.repeat(3_000_001)]);
  await assert.rejects(sessionFor(page).get('https://example.test/x'), /3 MB/);
});

test('a run attaches to the Chrome that is up, without starting another', async () => {
  let started = 0;
  const browser = await attach({ port: () => '9001', connect: async port => ({ port }), start: () => started++, wait: async () => {} });
  assert.deepEqual(browser, { port: '9001' });
  assert.equal(started, 0);
});

test('with no Chrome up (no port, or a port nothing answers on) a run starts one, once, and attaches when it is up', async () => {
  let started = 0;
  let waits = 0;
  const ports = [null, '9001', '9001', '9002'];
  const asked = [];
  const browser = await attach({
    port: () => ports.shift(),
    connect: async port => { asked.push(port); if (port !== '9002') throw new Error('ECONNREFUSED'); return { port }; },
    start: () => started++,
    wait: async () => { waits++; },
  });
  assert.deepEqual(browser, { port: '9002' });
  assert.deepEqual(asked, ['9001', '9001', '9002'], 'no connection is tried without a port');
  assert.equal(started, 1);
  assert.equal(waits, 3);
});

test('a Chrome that never comes up is an error, not a hang', async () => {
  let waits = 0;
  await assert.rejects(attach({ port: () => null, connect: async () => { throw new Error('unreachable'); }, start: () => {}, wait: async () => { waits++; }, tries: 5 }), /Chrome/);
  assert.equal(waits, 5);
});

test('the shared Chrome is kept until no run has had a tab open for the idle time', async () => {
  let now = 0;
  const open = [1, 2, 1, 2, 1, 1]; // tabs at each look: the Chrome's own first tab, plus the runs'
  const looks = [];
  await untilIdle(() => { looks.push(now); return open.shift() ?? 1; }, { idle: 20, every: 10, now: () => now, wait: async ms => { now += ms; } });
  assert.deepEqual(looks, [0, 10, 20, 30, 40, 50], 'a run had a tab at 10 and at 30; none for the idle time at 50');
});
