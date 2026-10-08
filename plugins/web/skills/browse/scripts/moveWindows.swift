// ABOUTME: Moves one process's windows that are off the named display (any part of its name, any case;
// ABOUTME: none = the main display) onto it, via the Accessibility API. `swift moveWindows.swift <pid> <margin> [name]`; prints moved|skip.
import AppKit
import ApplicationServices
let args = CommandLine.arguments
let pid = pid_t(args[1])!; let margin = CGFloat(Double(args[2])!)
let name = args.count > 3 ? args[3] : ""
var target: CGDirectDisplayID? = name.isEmpty ? CGMainDisplayID() : nil
for screen in NSScreen.screens where target == nil && screen.localizedName.localizedCaseInsensitiveContains(name) {
  target = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? CGDirectDisplayID
}
guard let id = target else { print("skip"); exit(0) }
let b = CGDisplayBounds(id)
let app = AXUIElementCreateApplication(pid)
var value: CFTypeRef?
guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value) == .success,
      let windows = value as? [AXUIElement], !windows.isEmpty else { print("skip"); exit(0) }
var point = CGPoint(x: b.origin.x + margin, y: b.origin.y + margin)
var moved = false
for w in windows {
  // A window already on the display stays where it is.
  var current: CFTypeRef?; var at = CGPoint.zero
  if AXUIElementCopyAttributeValue(w, kAXPositionAttribute as CFString, &current) == .success,
     AXValueGetValue(current as! AXValue, .cgPoint, &at), b.contains(at) { continue }
  if AXUIElementSetAttributeValue(w, kAXPositionAttribute as CFString, AXValueCreate(.cgPoint, &point)!) == .success { moved = true }
}
print(moved ? "moved" : "skip")
