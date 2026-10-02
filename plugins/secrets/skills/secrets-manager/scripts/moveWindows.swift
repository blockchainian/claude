// ABOUTME: Moves one process's windows that are off the built-in display onto it, via the Accessibility API.
// ABOUTME: Run by window-place.mjs as `swift moveWindows.swift <pid> <margin>`; prints moved|skip.
import AppKit
import ApplicationServices
let args = CommandLine.arguments
let pid = pid_t(args[1])!; let margin = CGFloat(Double(args[2])!)
var ids = [CGDirectDisplayID](repeating: 0, count: 16); var count: UInt32 = 0
guard CGGetActiveDisplayList(16, &ids, &count) == .success else { print("skip"); exit(0) }
var bounds: CGRect? = nil
for i in 0..<Int(count) where CGDisplayIsBuiltin(ids[i]) != 0 { bounds = CGDisplayBounds(ids[i]) }
guard let b = bounds, !(b.origin.x == 0 && b.origin.y == 0) else { print("skip"); exit(0) }
let app = AXUIElementCreateApplication(pid)
var value: CFTypeRef?
guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value) == .success,
      let windows = value as? [AXUIElement], !windows.isEmpty else { print("skip"); exit(0) }
var point = CGPoint(x: b.origin.x + margin, y: b.origin.y + margin)
var moved = false
for w in windows {
  // A window already on the built-in display stays where it is.
  var current: CFTypeRef?; var at = CGPoint.zero
  if AXUIElementCopyAttributeValue(w, kAXPositionAttribute as CFString, &current) == .success,
     AXValueGetValue(current as! AXValue, .cgPoint, &at), b.contains(at) { continue }
  if AXUIElementSetAttributeValue(w, kAXPositionAttribute as CFString, AXValueCreate(.cgPoint, &point)!) == .success { moved = true }
}
print(moved ? "moved" : "skip")
