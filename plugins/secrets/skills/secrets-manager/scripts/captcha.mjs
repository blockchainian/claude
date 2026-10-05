// ABOUTME: The DOM readiness check that gates classifying a reCAPTCHA image grid on Google's
// ABOUTME: sign-in path: a grid is classified only once every tile has fully rendered.

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
