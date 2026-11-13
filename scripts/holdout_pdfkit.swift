// macOS-only independent PDF producer and differential reference extractor.
// Generated fixtures are stress cases, not a blind real-world benchmark.
import Foundation
import CoreGraphics
import CoreText
import PDFKit

struct Span: Decodable {
    let text: String
    let x: Double
    let y: Double
    let size: Double
    let font: String
    let angle: Double?
}
struct Page: Decodable {
    let spans: [Span]
    let scan: Bool?
    let rules: [[Double]]?
}
struct Fixture: Decodable {
    let id: String
    let pages: [Page]
}
func paint(_ page: Page, _ context: CGContext) {
    context.setFillColor(CGColor(gray: 0, alpha: 1))
    for span in page.spans {
        context.saveGState()
        context.translateBy(x: span.x, y: span.y)
        context.rotate(by: (span.angle ?? 0) * Double.pi / 180)
        context.textMatrix = .identity
        context.textPosition = .zero
        let font = CTFontCreateWithName(span.font as CFString, span.size, nil)
        let attrs: [NSAttributedString.Key: Any] = [
            NSAttributedString.Key(kCTFontAttributeName as String): font,
            NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String): true
        ]
        let string = NSAttributedString(string: span.text, attributes: attrs)
        CTLineDraw(CTLineCreateWithAttributedString(string), context)
        context.restoreGState()
    }
    context.setStrokeColor(CGColor(gray: 0.3, alpha: 1))
    context.setLineWidth(0.5)
    for rule in page.rules ?? [] {
        context.move(to: CGPoint(x: rule[0], y: rule[1]))
        context.addLine(to: CGPoint(x: rule[2], y: rule[3]))
        context.strokePath()
    }
}
let args = CommandLine.arguments
if args[1] == "extract" {
    let doc = PDFDocument(url: URL(fileURLWithPath: args[2]))!
    let text = (0..<doc.pageCount).map { doc.page(at: $0)?.string ?? "" }.joined(separator: "\n\n")
    try text.write(toFile: args[3], atomically: true, encoding: .utf8)
} else {
    let fixtures = try JSONDecoder().decode([Fixture].self, from: Data(contentsOf: URL(fileURLWithPath: args[2])))
    try FileManager.default.createDirectory(atPath: args[3], withIntermediateDirectories: true)
    for fixture in fixtures {
        var box = CGRect(x: 0, y: 0, width: 612, height: 792)
        let url = URL(fileURLWithPath: args[3]).appendingPathComponent(fixture.id + ".pdf")
        let consumer = CGDataConsumer(url: url as CFURL)!
        let context = CGContext(consumer: consumer, mediaBox: &box, nil)!
        for page in fixture.pages {
            context.beginPDFPage(nil)
            if page.scan == true {
                let bitmap = CGContext(data: nil, width: 1224, height: 1584,
                    bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpaceCreateDeviceRGB(),
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
                bitmap.setFillColor(CGColor(gray: 1, alpha: 1))
                bitmap.fill(CGRect(x: 0, y: 0, width: 1224, height: 1584))
                bitmap.scaleBy(x: 2, y: 2)
                paint(page, bitmap)
                context.draw(bitmap.makeImage()!, in: box)
            } else { paint(page, context) }
            context.endPDFPage()
        }
        context.closePDF()
    }
}
