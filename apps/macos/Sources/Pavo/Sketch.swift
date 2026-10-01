import SwiftUI

/// The website's look: white pencil lines on near-black, with plain system text.
enum Ink {
    static let paper = Color(red: 0.047, green: 0.047, blue: 0.051)
    static let pencil = Color(red: 0.945, green: 0.937, blue: 0.910)
    static let faded = Color(red: 0.553, green: 0.545, blue: 0.518)

    /// Plain system text: easy to read at a glance.
    static func font(_ size: CGFloat, bold: Bool = true) -> Font {
        .system(size: size, weight: bold ? .semibold : .regular)
    }
}

/// A seeded random number generator (splitmix64), so a sketch comes out the same every time it's drawn.
nonisolated struct Wobble: RandomNumberGenerator {
    private var state: UInt64

    init(_ seed: UInt64) {
        state = seed
    }

    /// FNV-1a, so the same label always gets the same wobble.
    init(_ text: String) {
        self.init(text.utf8.reduce(0xcbf2_9ce4_8422_2325) { ($0 ^ UInt64($1)) &* 0x100_0000_01b3 })
    }

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    mutating func jitter(_ amount: CGFloat) -> CGFloat {
        CGFloat.random(in: -amount...amount, using: &self)
    }
}

nonisolated extension Path {
    /// A pencil stroke: two slightly different, slightly bowed passes, like rough.js.
    mutating func pencilLine(from a: CGPoint, to b: CGPoint, roughness: CGFloat, using rng: inout Wobble) {
        let length = hypot(b.x - a.x, b.y - a.y)
        guard length > 0 else { return }
        let normal = CGPoint(x: -(b.y - a.y) / length, y: (b.x - a.x) / length)
        for _ in 0..<2 {
            let start = CGPoint(x: a.x + rng.jitter(roughness), y: a.y + rng.jitter(roughness))
            let end = CGPoint(x: b.x + rng.jitter(roughness), y: b.y + rng.jitter(roughness))
            let bow = rng.jitter(min(length * 0.02, 2.2))
            let control = CGPoint(
                x: (start.x + end.x) / 2 + normal.x * bow + rng.jitter(roughness * 0.5),
                y: (start.y + end.y) / 2 + normal.y * bow + rng.jitter(roughness * 0.5)
            )
            move(to: start)
            addQuadCurve(to: end, control: control)
        }
    }

    /// A hand-drawn ellipse: two loops that don't quite meet.
    mutating func pencilEllipse(in rect: CGRect, roughness: CGFloat, using rng: inout Wobble) {
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let steps = 12
        for _ in 0..<2 {
            let start = Double(rng.jitter(.pi))
            let points = (0...steps).map { i -> CGPoint in
                let angle = start + Double(i) / Double(steps) * 2 * .pi * 1.04
                let wobble = 1 + rng.jitter(0.035)
                return CGPoint(
                    x: center.x + cos(angle) * rect.width / 2 * wobble,
                    y: center.y + sin(angle) * rect.height / 2 * wobble
                )
            }
            move(to: points[0])
            for i in 1..<points.count - 1 {
                let mid = CGPoint(x: (points[i].x + points[i + 1].x) / 2, y: (points[i].y + points[i + 1].y) / 2)
                addQuadCurve(to: mid, control: points[i])
            }
        }
    }
}

/// A rectangle drawn in pencil.
nonisolated struct SketchRect: Shape {
    var seed: String
    var roughness: CGFloat = 1.1

    func path(in rect: CGRect) -> Path {
        var rng = Wobble(seed)
        var path = Path()
        let r = rect.insetBy(dx: 1.5, dy: 1.5)
        let corners = [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.maxX, y: r.minY), CGPoint(x: r.maxX, y: r.maxY), CGPoint(x: r.minX, y: r.maxY)]
        for i in 0..<4 {
            path.pencilLine(from: corners[i], to: corners[(i + 1) % 4], roughness: roughness, using: &rng)
        }
        return path
    }
}

/// An ellipse drawn in pencil.
nonisolated struct SketchEllipse: Shape {
    var seed: String
    var roughness: CGFloat = 1

    func path(in rect: CGRect) -> Path {
        var rng = Wobble(seed)
        var path = Path()
        path.pencilEllipse(in: rect.insetBy(dx: 2, dy: 2), roughness: roughness, using: &rng)
        return path
    }
}

/// Diagonal pencil shading, rough.js's "hachure" fill.
nonisolated struct Hachure: Shape {
    var seed: String
    var gap: CGFloat = 5

    func path(in rect: CGRect) -> Path {
        var rng = Wobble(seed + "#")
        var path = Path()
        var x = rect.minX - rect.height
        while x < rect.maxX {
            let a = CGPoint(x: x, y: rect.maxY)
            let b = CGPoint(x: x + rect.height, y: rect.minY)
            path.move(to: CGPoint(x: a.x + rng.jitter(0.8), y: a.y))
            path.addLine(to: CGPoint(x: b.x + rng.jitter(0.8), y: b.y))
            x += gap
        }
        return path
    }
}

extension View {
    /// A pencil box around the view, optionally shaded in.
    func sketchBox(_ seed: String, shaded: Bool = false, dashed: Bool = false, ink: Color = Ink.pencil) -> some View {
        background {
            ZStack {
                if shaded {
                    Hachure(seed: seed).stroke(ink.opacity(0.28), lineWidth: 1).clipShape(Rectangle().inset(by: 3))
                }
                SketchRect(seed: seed)
                    .stroke(ink, style: StrokeStyle(lineWidth: 1.3, lineCap: .round, dash: dashed ? [5, 5] : []))
            }
        }
    }
}

/// A pencil-drawn button: a rough box that shades in while it's hovered or picked with the keyboard.
struct SketchButtonStyle: ButtonStyle {
    var seed: String
    var highlighted = false
    var size: CGFloat = 14

    func makeBody(configuration: Configuration) -> some View {
        SketchButtonBody(configuration: configuration, seed: seed, highlighted: highlighted, size: size)
    }
}

private struct SketchButtonBody: View {
    let configuration: ButtonStyleConfiguration
    let seed: String
    let highlighted: Bool
    let size: CGFloat
    @State private var hovering = false

    var body: some View {
        configuration.label
            .font(Ink.font(size))
            .foregroundStyle(Ink.pencil)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .frame(minHeight: 30)
            .sketchBox(seed, shaded: hovering || highlighted)
            .contentShape(Rectangle())
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
            .onHover { hovering = $0 }
    }
}
