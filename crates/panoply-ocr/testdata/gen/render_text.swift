// Renders paragraphs of text to gray PNG files with CoreText, which shapes Devanagari and
// Bengali conjuncts correctly. Usage: render_text <out_dir> <spec.tsv>
// spec.tsv lines: id <TAB> font <TAB> size_pt <TAB> width_px <TAB> text
import AppKit
import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers

let args = CommandLine.arguments
guard args.count == 3 else { fatalError("usage: render_text <out_dir> <spec.tsv>") }
let outDir = args[1]
let spec = try String(contentsOfFile: args[2], encoding: .utf8)
let dpi: CGFloat = 300

for line in spec.split(separator: "\n") where !line.hasPrefix("#") && !line.isEmpty {
    let f = line.split(separator: "\t", maxSplits: 4, omittingEmptySubsequences: false).map(String.init)
    guard f.count == 5 else { continue }
    let (id, fontName, sizePt, widthPx, text) = (f[0], f[1], CGFloat(Double(f[2])!), Int(f[3])!, f[4])
    let px = sizePt * dpi / 72
    let font = CTFontCreateWithName(fontName as CFString, px, nil)
    let actual = CTFontCopyPostScriptName(font) as String
    FileHandle.standardError.write("\(id): \(fontName) -> \(actual)\n".data(using: .utf8)!)
    let attrs: [NSAttributedString.Key: Any] = [
        .font: font,
        .foregroundColor: NSColor(calibratedWhite: 0.08, alpha: 1),
    ]
    let para = NSMutableParagraphStyle()
    para.lineSpacing = px * 0.25
    var a = attrs
    a[.paragraphStyle] = para
    let str = NSAttributedString(string: text, attributes: a)
    let setter = CTFramesetterCreateWithAttributedString(str)
    let margin: CGFloat = 90
    let textWidth = CGFloat(widthPx) - 2 * margin
    let fit = CTFramesetterSuggestFrameSizeWithConstraints(
        setter, CFRange(location: 0, length: 0), nil, CGSize(width: textWidth, height: 100000), nil)
    let height = Int(ceil(fit.height + 2 * margin))
    let cs = CGColorSpaceCreateDeviceGray()
    guard let ctx = CGContext(data: nil, width: widthPx, height: height, bitsPerComponent: 8,
                              bytesPerRow: 0, space: cs, bitmapInfo: CGImageAlphaInfo.none.rawValue) else { continue }
    ctx.setFillColor(CGColor(gray: 1, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: widthPx, height: height))
    let path = CGPath(rect: CGRect(x: margin, y: margin, width: textWidth, height: fit.height + 4), transform: nil)
    let frame = CTFramesetterCreateFrame(setter, CFRange(location: 0, length: 0), path, nil)
    CTFrameDraw(frame, ctx)
    guard let img = ctx.makeImage() else { continue }
    let url = URL(fileURLWithPath: "\(outDir)/\(id).png")
    guard let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else { continue }
    CGImageDestinationAddImage(dest, img, [kCGImagePropertyDPIWidth: 300, kCGImagePropertyDPIHeight: 300] as CFDictionary)
    CGImageDestinationFinalize(dest)
}
