// ABOUTME: Prints the built-in display's origin in global screen coordinates as "<x> <y>".
// ABOUTME: Run by window-place.mjs as `swift builtinDisplay.swift`; prints skip when there is none.
import CoreGraphics
var ids = [CGDirectDisplayID](repeating: 0, count: 16); var count: UInt32 = 0
guard CGGetActiveDisplayList(16, &ids, &count) == .success else { print("skip"); exit(0) }
for i in 0..<Int(count) where CGDisplayIsBuiltin(ids[i]) != 0 {
  let b = CGDisplayBounds(ids[i])
  print(Int(b.origin.x), Int(b.origin.y)); exit(0)
}
print("skip")
