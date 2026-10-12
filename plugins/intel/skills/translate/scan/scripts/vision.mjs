#!/usr/bin/env node
// ABOUTME: macOS Vision text recognition of a page image: the text lines with their boxes. Builds the small Swift
// ABOUTME: helper (vision_ocr.swift) into the data root on first use or when its source is newer.
//
// Usage: vision.mjs <image>   prints "x0 y0 x1 y1<TAB>text" per line, boxes in page fractions from the top left
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { dataDir } from "../../../fetch-x-mentions/scripts/env.mjs";

const SOURCE = path.join(path.dirname(fileURLToPath(import.meta.url)), "vision_ocr.swift");

/** The compiled helper, built with swiftc when missing or older than its source. */
export function visionBinary() {
  const bin = path.join(dataDir(), "translate-scan", "bin", "vision_ocr");
  if (!fs.existsSync(bin) || fs.statSync(bin).mtimeMs < fs.statSync(SOURCE).mtimeMs) {
    fs.mkdirSync(path.dirname(bin), { recursive: true });
    execFileSync("swiftc", ["-O", SOURCE, "-o", bin], { stdio: ["ignore", "ignore", "inherit"] });
  }
  return bin;
}

export function visionText(image) {
  return execFileSync(visionBinary(), [image], { encoding: "utf8", maxBuffer: 1 << 26 });
}

/** The page's text lines as macOS Vision reads them: [{box: [x0, y0, x1, y1] in page fractions, text}]. */
export function visionLines(image) {
  return visionText(image).split("\n").filter((l) => l.includes("\t")).map((l) => {
    const [box, text] = l.split("\t");
    return { box: box.split(" ").map(Number), text };
  });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  process.stdout.write(visionText(path.resolve(process.argv[2])));
}
