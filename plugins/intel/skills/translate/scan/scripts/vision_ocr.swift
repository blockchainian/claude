// Prints the macOS Vision OCR of an image, one recognised line per output line in reading order, as
// "x0 y0 x1 y1<TAB>text" with the line's box in fractions of the image, origin at the top left.
import Foundation
import Vision
import AppKit

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let img = NSImage(contentsOf: url),
      let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else { exit(1) }
let req = VNRecognizeTextRequest()
req.recognitionLevel = .accurate
req.usesLanguageCorrection = true
try VNImageRequestHandler(cgImage: cg).perform([req])
for obs in req.results ?? [] {
    if let top = obs.topCandidates(1).first {
        let b = obs.boundingBox
        print(String(format: "%.4f %.4f %.4f %.4f\t", b.minX, 1 - b.maxY, b.maxX, 1 - b.minY) + top.string)
    }
}
