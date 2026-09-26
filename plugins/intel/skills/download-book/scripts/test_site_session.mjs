// ABOUTME: Tests the session wrapper's challenge handling against a fake page — no network:
// ABOUTME: a challenge body triggers one navigation and one refetch; other bodies pass through.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sessionFor, isChallenge } from './site_session.mjs';

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
