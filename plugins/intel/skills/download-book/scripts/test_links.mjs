// ABOUTME: Tests slow-download entry selection: the fast waitlist server is chosen over the throttled no-waitlist one.
// ABOUTME: Uses a small inline fixture mirroring the site's per-<li> option list; no network.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseLinks, selectSlowPaths } from './anna_archive_links.mjs';

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
