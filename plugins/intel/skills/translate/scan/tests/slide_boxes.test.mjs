// ABOUTME: Tests the slide text boxes on a real 6.1910 slide: Vision's duplicate line tail is dropped, bullets stay
// ABOUTME: separate boxes, and a box takes the codex transcription's text when the two match.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { groupLines, mergeRows, paragraphStarts, transcribedText } from "../scripts/slide_boxes.mjs";

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures");
const md = fs.readFileSync(path.join(dir, "slide-6.1910-L02-p08.md"), "utf8");
const lines = fs.readFileSync(path.join(dir, "slide-6.1910-L02-p08.vision.txt"), "utf8").split("\n")
  .filter((l) => l.includes("\t")).map((l) => {
    const [box, text] = l.split("\t");
    return { box: box.split(" ").map(Number), text, inFigure: false };
  });

test("a second read of a line's tail is dropped", () => {
  const texts = mergeRows(lines).map((l) => l.text);
  assert.ok(texts.some((t) => t.startsWith("Key idea")));
  assert.ok(!texts.includes(', "1"'));
});

test("the two bullets of the slide are two boxes", () => {
  const boxes = groupLines(mergeRows(lines), paragraphStarts(md)).map((b) => b.lines.map((l) => l.text).join(" "));
  assert.ok(boxes.some((t) => t.startsWith("Key idea")) && boxes.some((t) => t.startsWith("Use the same")));
  assert.ok(!boxes.some((t) => t.includes("Key idea") && t.includes("Use the same")));
});

test("a box whose words match a transcribed paragraph takes its text", () => {
  assert.match(transcribedText("Use the same uniform representation convention for every component and wire in our digital system", md), /\*every\*/);
  assert.equal(transcribedText("Attempt #3: something else entirely", md), null);
});
