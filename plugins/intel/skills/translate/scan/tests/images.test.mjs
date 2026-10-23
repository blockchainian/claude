import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { dimensions, prepareImage } from "../scripts/images.mjs";

const fixture = new URL("./fixtures/handwritten-6.1220-L03-p02.png", import.meta.url);

test("a real handwritten PNG within the image budget retains every byte", async () => {
  assert.deepEqual(dimensions(850, 1100), [850, 1100]);
  assert.deepEqual(await prepareImage(fixture), await fs.readFile(fixture));
});

test("the real pilot page 8 dimensions fit the CLI high-detail patch budget", () => {
  // Measured from the pilot's p-08.png; the PDF is not a repository fixture.
  assert.deepEqual(dimensions(2550, 3300), [1376, 1780]);
});

test("oversized real image pixels are resized exactly and remain unchanged on another pass", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "scan-images-"));
  try {
    const enlarged = path.join(dir, "enlarged.png");
    const prepared = path.join(dir, "prepared.png");
    execFileSync("magick", [fixture.pathname, "-resize", "2550x3300!", enlarged]);
    const bytes = await prepareImage(enlarged);
    const width = bytes.readUInt32BE(16);
    const height = bytes.readUInt32BE(20);
    assert.deepEqual([width, height], [1376, 1780]);
    assert.ok(Math.max(width, height) <= 2048);
    assert.ok(Math.ceil(width / 32) * Math.ceil(height / 32) <= 2500);
    const { imageInput } = await import("../../epub/scripts/codex.mjs");
    const content = imageInput("Read.", [enlarged], [bytes])[0].content;
    assert.ok(Buffer.from(content.find(part => part.type === "input_image").image_url.split(",")[1], "base64").equals(bytes));
    assert.ok(content[0].text.includes(enlarged));
    await fs.writeFile(prepared, bytes);
    assert.deepEqual(await prepareImage(prepared), bytes);
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
});

test("invalid required PNG dimensions fail", async () => {
  for (const value of [0, -1, 1.5, NaN, Infinity, 2 ** 32]) {
    assert.throws(() => dimensions(value, 1100), /dimensions/);
    assert.throws(() => dimensions(850, value), /dimensions/);
  }
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "scan-images-"));
  try {
    const invalid = path.join(dir, "invalid.png");
    const bytes = await fs.readFile(fixture);
    bytes.writeUInt32BE(0, 16);
    await fs.writeFile(invalid, bytes);
    await assert.rejects(prepareImage(invalid), /dimensions/);
    await fs.writeFile(invalid, Buffer.from("not a PNG"));
    await assert.rejects(prepareImage(invalid), /PNG/);
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
});
