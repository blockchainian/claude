// ABOUTME: Prints the origin, in global screen coordinates, of the display named by the argument (any
// ABOUTME: part of its name, any case; none = the main display) as "<x> <y>", or skip when it is not connected.
import AppKit
let name = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ""
var target: CGDirectDisplayID? = name.isEmpty ? CGMainDisplayID() : nil
for screen in NSScreen.screens where target == nil && screen.localizedName.localizedCaseInsensitiveContains(name) {
  target = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? CGDirectDisplayID
}
guard let id = target else { print("skip"); exit(0) }
let b = CGDisplayBounds(id)
print(Int(b.origin.x), Int(b.origin.y))
