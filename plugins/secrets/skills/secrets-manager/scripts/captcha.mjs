// ABOUTME: CapSolver client for the password-page distorted-text CAPTCHA on Google's sign-in path,
// ABOUTME: plus the DOM readiness check that gates classifying a reCAPTCHA image grid.

import * as config from "./config.mjs";

const API_URL = "https://api.capsolver.com";

// --- pure builders / parsers (tested) ----------------------------------------

// The CapSolver task for the old-style distorted-text image on the password page.
export function buildImageToTextTask(base64) {
  if (!base64) throw new Error("buildImageToTextTask needs a base64 image");
  return { type: "ImageToText", module: "common", body: base64 };
}

// Decide whether a reCAPTCHA image grid is fully rendered and safe to classify/click, from one record
// per tile read out of the DOM: { hasImg, complete, naturalWidth, opacity }. A grid is ready only when
// every tile owns an image that has finished downloading (`complete`), has real pixels
// (`naturalWidth > 0`), and has finished its fade-in (`opacity` at 1). A missing image (a cell caught
// mid-swap) or any grainy/half-faded tile makes the whole grid not-ready — classifying it is what made
// the solver click blank or wrong cells. `records` empty → not ready (no grid to judge).
export function gridReady(records) {
  if (!Array.isArray(records) || records.length === 0) return false;
  return records.every(
    (r) => r && r.hasImg && r.complete && r.naturalWidth > 0 && r.opacity >= 0.99,
  );
}

// --- network (not unit-tested; verified live) --------------------------------

function requireKey() {
  const key = config.capSolverKey();
  if (!key) throw new Error("No CapSolver key. Add CAPSOLVER_API_KEY to ~/.config/secrets-manager/.env.");
  return key;
}

async function post(path, body) {
  const res = await fetch(`${API_URL}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return res.json();
}

// Recognition tasks (image-to-text) are synchronous: createTask returns the solution in the same
// response, so there is nothing to poll — polling getTaskResult for one only hits ERROR_TASK_NOT_FOUND
// once its short-lived id expires. Returns the solution object.
async function solveTaskSync(task) {
  const clientKey = requireKey();
  const json = await post("/createTask", { clientKey, task });
  if (!json || json.errorId) {
    throw new Error(`CapSolver createTask error: ${json?.errorCode || "unknown"} ${json?.errorDescription || ""}`.trim());
  }
  if (json.solution) return json.solution;
  throw new Error("CapSolver returned no solution for the recognition task");
}

// Read the distorted text off a password-page image (base64, no data: prefix).
export async function solveImageToText(base64) {
  const solution = await solveTaskSync(buildImageToTextTask(base64));
  const text = solution.text;
  if (!text) throw new Error("CapSolver returned no text for the image CAPTCHA");
  return text;
}
