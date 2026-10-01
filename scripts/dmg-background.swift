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

            // a box around Pavo, like the website's sketch
            let box = CGRect(x: Layout.app.x - 78, y: Layout.app.y - 84, width: 156, height: 156)
            context.stroke(SketchRect(seed: "dmg-app", roughness: 1.3).path(in: box), with: .color(Ink.pencil), style: pencil)

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

            context.draw(
                Text("drag it over").font(.custom("Caveat", size: 34).weight(.bold)).foregroundStyle(Ink.faded),
                at: CGPoint(x: Layout.window.width / 2, y: 336)
            )
        }
        .frame(width: Layout.window.width, height: Layout.window.height)
        .background(Ink.paper)
    }
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
