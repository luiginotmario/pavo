import AppKit

/// A peacock's fanned tail, drawn as a template image so macOS tints it for light and dark menu bars.
enum PeacockIcon {
    static func make() -> NSImage {
        let image = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { _ in
            let base = NSPoint(x: 9, y: 2.6)
            NSColor.black.set()
            for i in 0..<5 {
                let angle = (150.0 - Double(i) * 30.0) * .pi / 180
                let tip = NSPoint(x: base.x + cos(angle) * 7.4, y: base.y + sin(angle) * 12)
                let feather = NSBezierPath()
                feather.move(to: base)
                feather.line(to: tip)
                feather.lineWidth = 1.3
                feather.lineCapStyle = .round
                feather.stroke()
                NSBezierPath(ovalIn: NSRect(x: tip.x - 1.7, y: tip.y - 1.7, width: 3.4, height: 3.4)).fill()
            }
            NSBezierPath(ovalIn: NSRect(x: base.x - 2.1, y: base.y - 2.1, width: 4.2, height: 4.2)).fill()
            return true
        }
        image.isTemplate = true
        image.accessibilityDescription = "pavo"
        return image
    }
}
