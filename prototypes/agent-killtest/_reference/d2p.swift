import AppKit
let args = CommandLine.arguments
let url = URL(fileURLWithPath: args[1])
let s = try! NSAttributedString(url: url, options: [:], documentAttributes: nil)
let tv = NSTextView(frame: NSRect(x: 0, y: 0, width: 540, height: 100))
tv.textStorage!.setAttributedString(s)
tv.sizeToFit()
let data = tv.dataWithPDF(inside: tv.bounds)
try! data.write(to: URL(fileURLWithPath: args[2]))
