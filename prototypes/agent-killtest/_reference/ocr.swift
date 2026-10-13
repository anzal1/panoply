import Vision
import AppKit
let dir = CommandLine.arguments[1]
for f in try! FileManager.default.contentsOfDirectory(atPath: dir).sorted() {
  guard let img = NSImage(contentsOfFile: dir + "/" + f), let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else { continue }
  let req = VNRecognizeTextRequest(); req.recognitionLevel = .accurate
  try? VNImageRequestHandler(cgImage: cg).perform([req])
  let t = (req.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
  if t.contains("ORDER-4471") { print(f) }
}
