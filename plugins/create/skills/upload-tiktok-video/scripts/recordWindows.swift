// ABOUTME: Records every window of one process to a .mov via ScreenCaptureKit, until SIGINT.
// ABOUTME: Run by debug.mjs as `swift recordWindows.swift <pid> <out.mov>`; prints recording|skip.
import AppKit
import ScreenCaptureKit

let args = CommandLine.arguments
let pid = pid_t(args[1])!
let outURL = URL(fileURLWithPath: args[2])

func skip(_ why: String) -> Never {
  print("skip: \(why)")
  exit(0)
}

final class RecordingEvents: NSObject, SCRecordingOutputDelegate {}
let events = RecordingEvents()
var stream: SCStream?
var output: SCRecordingOutput?

// Ignore the default SIGINT action so the stream can be stopped and the .mov finalized first.
signal(SIGINT, SIG_IGN)
let sigint = DispatchSource.makeSignalSource(signal: SIGINT, queue: .main)
sigint.setEventHandler {
  Task {
    try? await stream?.stopCapture()
    exit(0)
  }
}
sigint.resume()

// ScreenCaptureKit builds its menu-bar recording indicator while starting, which needs the main thread.
let nsApp = NSApplication.shared
nsApp.setActivationPolicy(.accessory)
Task { @MainActor in
  do {
    let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
    guard let app = content.applications.first(where: { $0.processID == pid }) else { skip("no app for pid") }
    // Record the display the process's largest window sits on; its other windows (an OAuth popup
    // opened later) join the same recording because the filter is by application.
    let windows = content.windows.filter { $0.owningApplication?.processID == pid && $0.windowLayer == 0 }
    let largest = windows.max { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }
    let center = largest.map { CGPoint(x: $0.frame.midX, y: $0.frame.midY) }
    guard let display = content.displays.first(where: { d in center.map { d.frame.contains($0) } ?? false })
      ?? content.displays.first else { skip("no display") }

    let filter = SCContentFilter(display: display, including: [app], exceptingWindows: [])
    let config = SCStreamConfiguration()
    let scale = CGFloat(filter.pointPixelScale)
    config.width = Int(CGFloat(display.width) * scale)
    config.height = Int(CGFloat(display.height) * scale)
    config.showsCursor = true
    config.minimumFrameInterval = CMTime(value: 1, timescale: 15)

    let recording = SCRecordingOutputConfiguration()
    recording.outputURL = outURL
    recording.outputFileType = .mov
    recording.videoCodecType = .h264

    let s = SCStream(filter: filter, configuration: config, delegate: nil)
    let o = SCRecordingOutput(configuration: recording, delegate: events)
    try s.addRecordingOutput(o)
    try await s.startCapture()
    stream = s
    output = o
    print("recording")
    fflush(stdout)
  } catch {
    skip("\(error)")
  }
}

nsApp.run()
