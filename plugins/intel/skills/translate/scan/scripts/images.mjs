import { execFile } from "node:child_process";
import fs from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);
const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const patchSize = 32;
const maxDimension = 2048;
const maxPatches = 2500;

function fits(width, height) {
  return Math.max(width, height) <= maxDimension
    && Math.ceil(width / patchSize) * Math.ceil(height / patchSize) <= maxPatches;
}

// Matches Codex HIGH_DETAIL's dimension limit and Responses patch-budget rounding.
export function dimensions(width, height) {
  if (![width, height].every((value) => Number.isInteger(value) && value > 0 && value <= 0x7fffffff)) {
    throw new Error(`Invalid PNG dimensions: ${width}x${height}`);
  }
  if (fits(width, height)) return [width, height];
  const dimensionScale = Math.min(maxDimension / Math.max(width, height), 1);
  width = Math.max(1, Math.round(width * dimensionScale));
  height = Math.max(1, Math.round(height * dimensionScale));
  if (fits(width, height)) return [width, height];

  let scale = Math.sqrt(patchSize * patchSize * maxPatches / width / height);
  const patchesWide = width * scale / patchSize;
  const patchesHigh = height * scale / patchSize;
  scale *= Math.min(Math.floor(patchesWide) / patchesWide, Math.floor(patchesHigh) / patchesHigh);
  return [Math.max(1, Math.floor(width * scale)), Math.max(1, Math.floor(height * scale))];
}

export async function prepareImage(file) {
  const bytes = await fs.readFile(file);
  if (bytes.length < 33 || !bytes.subarray(0, 8).equals(signature)
      || bytes.readUInt32BE(8) !== 13 || bytes.toString("ascii", 12, 16) !== "IHDR") {
    throw new Error(`Invalid PNG header: ${file}`);
  }
  const width = bytes.readUInt32BE(16);
  const height = bytes.readUInt32BE(20);
  const [targetWidth, targetHeight] = dimensions(width, height);
  if (targetWidth === width && targetHeight === height) return bytes;
  const source = file instanceof URL ? fileURLToPath(file) : file;
  const { stdout } = await run("magick", [source, "-filter", "Triangle", "-resize", `${targetWidth}x${targetHeight}!`, "PNG:-"], {
    encoding: "buffer",
    maxBuffer: 64 * 1024 * 1024,
  });
  return stdout;
}
