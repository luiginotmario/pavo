import SwiftUI

/// The website's peacock, drawn with the same pencil.
struct Peacock: View {
    var body: some View {
        Canvas { context, size in
            let scale = min(size.width / 320, size.height / 236)
            context.translateBy(x: (size.width - 320 * scale) / 2, y: (size.height - 236 * scale) / 2)
            context.scaleBy(x: scale, y: scale)
            Self.draw(in: &context)
        }
        .accessibilityLabel("a hand-drawn peacock")
    }

    private static func draw(in context: inout GraphicsContext) {
        var rng = Wobble(7)
        let pencil = StrokeStyle(lineWidth: 1.4, lineCap: .round)
        let base = CGPoint(x: 160, y: 172)

        func feather(_ degrees: Double, length: Double, eye: CGSize) {
            let angle = degrees * .pi / 180
            let tip = CGPoint(x: base.x + cos(angle) * length, y: base.y + sin(angle) * length)
            var stem = Path()
            stem.pencilLine(from: base, to: tip, roughness: 1, using: &rng)
            context.stroke(stem, with: .color(Ink.pencil), style: pencil)

            var eyeContext = context
            eyeContext.translateBy(x: tip.x, y: tip.y)
            eyeContext.rotate(by: .radians(angle + .pi / 2))
            var outline = Path()
            outline.pencilEllipse(in: CGRect(x: -eye.width / 2, y: -eye.height / 2, width: eye.width, height: eye.height), roughness: 1, using: &rng)
            eyeContext.fill(Path(ellipseIn: CGRect(x: -eye.width / 2, y: -eye.height / 2, width: eye.width, height: eye.height)), with: .color(Ink.paper))
            eyeContext.stroke(outline, with: .color(Ink.pencil), style: pencil)
            let pupil = CGSize(width: eye.width * 0.42, height: eye.height * 0.42)
            eyeContext.fill(Path(ellipseIn: CGRect(x: -pupil.width / 2, y: -pupil.height / 2 + 2, width: pupil.width, height: pupil.height)), with: .color(Ink.pencil))
        }

        for i in 0..<9 { feather(-166 + Double(i) * 19, length: 126, eye: CGSize(width: 22, height: 30)) }
        for i in 0..<8 { feather(-156.5 + Double(i) * 19, length: 80, eye: CGSize(width: 14, height: 19)) }

        // body, neck and head sit in front of the tail
        let bodyRect = CGRect(x: 138, y: 156, width: 44, height: 56)
        var body = Path()
        body.pencilEllipse(in: bodyRect, roughness: 1, using: &rng)
        context.fill(Path(ellipseIn: bodyRect), with: .color(Ink.paper))
        context.stroke(body, with: .color(Ink.pencil), style: pencil)

        var neck = Path()
        neck.move(to: CGPoint(x: 150, y: 168))
        neck.addCurve(to: CGPoint(x: 166, y: 104), control1: CGPoint(x: 148, y: 142), control2: CGPoint(x: 158, y: 122))
        neck.addLine(to: CGPoint(x: 177, y: 107))
        neck.addCurve(to: CGPoint(x: 171, y: 168), control1: CGPoint(x: 172, y: 126), control2: CGPoint(x: 172, y: 146))
        context.fill(neck, with: .color(Ink.paper))
        context.stroke(neck, with: .color(Ink.pencil), style: pencil)

        let headRect = CGRect(x: 163.5, y: 89.5, width: 19, height: 19)
        var head = Path()
        head.pencilEllipse(in: headRect, roughness: 0.6, using: &rng)
        context.fill(Path(ellipseIn: headRect), with: .color(Ink.paper))
        context.stroke(head, with: .color(Ink.pencil), style: pencil)
        context.fill(Path(ellipseIn: CGRect(x: 174.5, y: 95.5, width: 3, height: 3)), with: .color(Ink.pencil))

        var details = Path()
        details.pencilLine(from: CGPoint(x: 181, y: 96), to: CGPoint(x: 192, y: 100), roughness: 0.5, using: &rng)
        details.pencilLine(from: CGPoint(x: 192, y: 100), to: CGPoint(x: 181, y: 104), roughness: 0.5, using: &rng)
        for crest in [CGPoint(x: 163, y: 76), CGPoint(x: 170, y: 73), CGPoint(x: 177, y: 76)] {
            details.pencilLine(from: CGPoint(x: 171, y: 90), to: crest, roughness: 0.5, using: &rng)
            context.fill(Path(ellipseIn: CGRect(x: crest.x - 2, y: crest.y - 2, width: 4, height: 4)), with: .color(Ink.pencil))
        }
        details.pencilLine(from: CGPoint(x: 153, y: 209), to: CGPoint(x: 151, y: 228), roughness: 0.8, using: &rng)
        details.pencilLine(from: CGPoint(x: 167, y: 209), to: CGPoint(x: 169, y: 228), roughness: 0.8, using: &rng)
        details.pencilLine(from: CGPoint(x: 144, y: 230), to: CGPoint(x: 156, y: 230), roughness: 0.6, using: &rng)
        details.pencilLine(from: CGPoint(x: 164, y: 230), to: CGPoint(x: 176, y: 230), roughness: 0.6, using: &rng)
        context.stroke(details, with: .color(Ink.pencil), style: pencil)
    }
}
