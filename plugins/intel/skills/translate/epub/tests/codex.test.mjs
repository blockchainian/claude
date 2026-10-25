import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { imageInput, textInput, requestInput } from '../scripts/codex.mjs';

const image = new URL('../../scan/tests/fixtures/handwritten-6.1220-L03-p02.png', import.meta.url);
test('page bytes become Responses image input in page order', () => {
  const input = imageInput('Read the page.', [image, image]);
  assert.equal(input[0].role, 'user');
  assert.match(input[0].content[0].text, /^<image name=\[Image #1\] path=/);
  assert.equal(input[0].content.at(-1).text, 'Read the page.');
  assert.equal(input[0].content.length, 7);
  for (const part of input[0].content.filter(part => part.type === 'input_image')) {
    assert.equal(part.type, 'input_image');
    assert.equal(Object.hasOwn(part, 'detail'), false);
    assert.deepEqual(Buffer.from(part.image_url.split(',')[1], 'base64'), fs.readFileSync(image));
    assert.ok(part.image_url.startsWith('data:image/png;base64,'));
  }
});
test('missing images fail before a request', () => {
  assert.throws(() => imageInput('Read.', ['/no-such-page.png']), /ENOENT/);
  assert.throws(() => imageInput('Read.', []), /image/i);
});

test('text input is an explicit user message without extra context', () => {
  assert.deepEqual(textInput('Book: Test\n\nHello.'), [{ role: 'user', content: [{ type: 'input_text', text: 'Book: Test\n\nHello.' }] }]);
});

test('rules use the CLI developer-message field without environment context', () => {
  assert.deepEqual(requestInput('Read.', 'Rules.\n'), [{ role: 'developer', content: [{ type: 'input_text', text: 'Rules.' }] }, ...textInput('Read.')]);
});
