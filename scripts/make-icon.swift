// Procedural app icon; the app itself is implemented in Rust.
import AppKit
let root = URL(fileURLWithPath: CommandLine.arguments[1])
let dir = root.appendingPathComponent("AppIcon.iconset")
try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = size * scale
        let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        let transform = NSAffineTransform(); transform.scale(by: CGFloat(pixels) / 512); transform.concat()
        let rect = NSRect(x: 26, y: 26, width: 460, height: 460)
        NSColor(calibratedRed: 0.07, green: 0.10, blue: 0.14, alpha: 1).setFill()
        NSBezierPath(roundedRect: rect, xRadius: 104, yRadius: 104).fill()
        NSColor(calibratedRed: 0.17, green: 0.23, blue: 0.30, alpha: 1).setStroke()
        let border = NSBezierPath(roundedRect: rect.insetBy(dx: 2, dy: 2), xRadius: 102, yRadius: 102)
        border.lineWidth = 3; border.stroke()
        for y in [170, 256, 342] {
            let line = NSBezierPath(); line.move(to: NSPoint(x: 90, y: y)); line.line(to: NSPoint(x: 422, y: y))
            line.lineWidth = 2; line.stroke()
        }
        NSColor(calibratedRed: 0.40, green: 0.88, blue: 0.72, alpha: 1).setStroke()
        let path = NSBezierPath(); path.move(to: NSPoint(x: 96, y: 248))
        for (x,y) in [(166,248), (212,366), (274,147), (322,280), (412,280)] {
            path.line(to: NSPoint(x:x, y:y))
        }
        path.lineWidth = 24; path.lineJoinStyle = .round; path.lineCapStyle = .round; path.stroke()
        NSGraphicsContext.restoreGraphicsState()
        let filename = "icon_\(size)x\(size)\(scale == 2 ? "@2x" : "").png"
        try rep.representation(using: .png, properties: [:])!.write(to: dir.appendingPathComponent(filename))
    }
}
