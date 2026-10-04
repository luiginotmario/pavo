// Draws the install window's background: a pencil box where Pavo sits, an arrow to the
// Applications folder, and "drag it over". Finder puts the real icons on top.
//
//   swiftc -parse-as-library scripts/dmg-background.swift apps/macos/Sources/Pavo/Sketch.swift -o /tmp/dmg-bg
//   /tmp/dmg-bg path/to/Caveat.ttf apps/macos/dmg
import AppKit
import CoreText
import SwiftUI

/// Where Finder places the icons (centres, in window points). make-dmg.sh uses the same numbers.
enum Layout {
    static let window = CGSize(width: 640, height: 440)
    static let app = CGPoint(x: 170, y: 190)
    static let applications = CGPoint(x: 470, y: 190)
}

struct Background: View {
    var body: some View {
        Canvas { context, _ in
            var rng = Wobble(11)
            let pencil = StrokeStyle(lineWidth: 1.6, lineCap: .round, lineJoin: .round)

            // the arrow over the top
            var arrow = Path()
            let start = CGPoint(x: Layout.app.x + 98, y: Layout.app.y - 30)
            let end = CGPoint(x: Layout.applications.x - 92, y: Layout.applications.y - 22)
            for _ in 0..<2 {
                arrow.move(to: CGPoint(x: start.x + rng.jitter(1.2), y: start.y + rng.jitter(1.2)))
                arrow.addQuadCurve(
                    to: CGPoint(x: end.x + rng.jitter(1), y: end.y + rng.jitter(1)),
                    control: CGPoint(x: (start.x + end.x) / 2 + rng.jitter(2), y: start.y - 70 + rng.jitter(2))
                )
            }
            let head = atan2(end.y - (start.y - 70), end.x - (start.x + end.x) / 2)
            for side in [0.8, -0.8] {
                let a = head + .pi * side
                arrow.pencilLine(from: end, to: CGPoint(x: end.x + cos(a) * 16, y: end.y + sin(a) * 16), roughness: 0.6, using: &rng)
            }
            context.stroke(arrow, with: .color(Ink.pencil), style: pencil)

            // a hand that's just grabbed Pavo, leaning toward Applications
            drawGrabbingHand(in: &context, at: CGPoint(x: Layout.app.x + 12, y: Layout.app.y - 86), tilt: 200, rng: &rng)

            context.draw(
                Text("drag it over").font(.custom("Caveat", size: 34).weight(.bold)).foregroundStyle(Ink.faded),
                at: CGPoint(x: Layout.window.width / 2, y: 336)
            )
            // the one thing worth knowing before you start
            context.draw(
                Text("then: drag any file and pause for a moment").font(.custom("Caveat", size: 24).weight(.medium)).foregroundStyle(Ink.faded.opacity(0.85)),
                at: CGPoint(x: Layout.window.width / 2, y: 376)
            )
        }
        .frame(width: Layout.window.width, height: Layout.window.height)
        .background(Ink.paper)
    }
}

/// The "grabbing" hand cursor, drawn in pencil: knuckles on top, fingers folded, the thumb pressing in, a wrist.
func drawGrabbingHand(in context: inout GraphicsContext, at center: CGPoint, tilt: Double, rng: inout Wobble) {
    // drawn in a 48×60 box, then placed and tilted
    var c = context
    c.translateBy(x: center.x, y: center.y)
    c.rotate(by: .degrees(tilt))
    c.scaleBy(x: 1.35, y: 1.35)
    c.translateBy(x: -24, y: -30)

    var hand = Path()
    hand.move(to: CGPoint(x: 9, y: 16))
    for i in 0..<4 { // four knuckles across the top
        let x = 9 + CGFloat(i) * 8.5
        hand.addQuadCurve(to: CGPoint(x: x + 8.5, y: 16), control: CGPoint(x: x + 4.25, y: 4))
    }
    hand.addQuadCurve(to: CGPoint(x: 45, y: 34), control: CGPoint(x: 48, y: 20))   // outside of the hand
    hand.addQuadCurve(to: CGPoint(x: 35, y: 48), control: CGPoint(x: 44, y: 45))
    hand.addLine(to: CGPoint(x: 35, y: 58))                                      // wrist
    hand.move(to: CGPoint(x: 17, y: 58))
    hand.addLine(to: CGPoint(x: 17, y: 49))
    hand.addQuadCurve(to: CGPoint(x: 4, y: 33), control: CGPoint(x: 6, y: 45))   // heel of the palm
    hand.addQuadCurve(to: CGPoint(x: 1, y: 22), control: CGPoint(x: -1, y: 27))  // thumb, sticking out
    hand.addQuadCurve(to: CGPoint(x: 9, y: 16), control: CGPoint(x: 3, y: 15))

    var fill = Path()
    fill.addRoundedRect(in: CGRect(x: 2, y: 9, width: 44, height: 46), cornerSize: CGSize(width: 14, height: 14))

    var details = Path()
    for i in 1..<4 { // where the fingers fold in
        let x = 9 + CGFloat(i) * 8.5
        details.pencilLine(from: CGPoint(x: x, y: 15), to: CGPoint(x: x + 0.5, y: 27), roughness: 0.3, using: &rng)
    }
    details.pencilLine(from: CGPoint(x: 9, y: 27), to: CGPoint(x: 37, y: 28), roughness: 0.4, using: &rng)  // fingertips tucked in
    details.pencilLine(from: CGPoint(x: 3, y: 24), to: CGPoint(x: 16, y: 31), roughness: 0.3, using: &rng)  // thumb over the fingers

    c.fill(fill, with: .color(Ink.paper))
    let pencil = StrokeStyle(lineWidth: 1.3, lineCap: .round, lineJoin: .round)
    for _ in 0..<2 { // two slightly different passes, like the rest of the sketch
        var pass = c
        pass.translateBy(x: rng.jitter(0.5), y: rng.jitter(0.5))
        pass.stroke(hand, with: .color(Ink.pencil), style: pencil)
    }
    c.stroke(details, with: .color(Ink.pencil), style: StrokeStyle(lineWidth: 1.1, lineCap: .round))
}

@main
struct Main {
    @MainActor static func main() throws {
        let args = CommandLine.arguments
        guard args.count == 3 else {
            print("usage: dmg-bg <Caveat.ttf> <output folder>")
            exit(1)
        }
        CTFontManagerRegisterFontsForURL(URL(fileURLWithPath: args[1]) as CFURL, .process, nil)
        let folder = URL(fileURLWithPath: args[2])
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for (scale, name) in [(1.0, "background.png"), (2.0, "background@2x.png")] {
            let renderer = ImageRenderer(content: Background())
            renderer.scale = scale
            guard let tiff = renderer.nsImage?.tiffRepresentation,
                  let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:])
            else { throw CocoaError(.fileWriteUnknown) }
            try png.write(to: folder.appending(path: name))
        }
        print("✓ \(folder.path)")
    }
}
