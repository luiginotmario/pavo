import AppKit
import SwiftUI

/// What the wheel shows while it's open.
@Observable
final class WheelModel {
    enum Mode {
        /// opened by ⇧-dragging: drop on a bubble, hold ⌥ for tools
        case drag
        /// opened from the right-click menu: click a bubble, click the middle for tools
        case click
    }

    var mode = Mode.drag
    var urls: [URL] = []
    var actions: [Engine.Action] = []
    var showingTools = false
    var hovered: String?

    var visible: [Engine.Action] {
        let wanted = showingTools ? actions.filter { $0.group != "convert" } : actions.filter { $0.group == "convert" }
        // trim needs times typed in, which the wheel can't do; the panel handles it
        return Array(wanted.filter { $0.id != "trim" }.prefix(14))
    }

    var hint: String {
        switch (actions.isEmpty, mode, showingTools) {
        case (true, _, _): "reading…"
        case (false, .drag, false): "hold ⌥ for tools"
        case (false, .drag, true): "let go of ⌥ for formats"
        case (false, .click, false): "click here for tools"
        case (false, .click, true): "click here for formats"
        }
    }
}

/// A wheel of formats around the pointer. Two ways in:
/// - hold ⇧ while dragging files anywhere, then drop on a bubble (⌥ switches to tools)
/// - right-click files → Convert with Pavo, then click a bubble
///
/// Watching drags needs no special permission: only mouse events are monitored, and only the
/// shift key is checked until a drag actually carries files.
final class Wheel {
    private let converter: Converter
    private let model = WheelModel()
    private var panel: FloatPanel?
    private var watchers: [Any] = []
    private var dismissal: Any?
    /// The drag pasteboard's change count when we last looked, so each drag is only picked up once.
    private var seenDrag = NSPasteboard(name: .drag).changeCount

    init(converter: Converter) {
        self.converter = converter
        watchers = [
            NSEvent.addGlobalMonitorForEvents(matching: .leftMouseDragged) { _ in
                MainActor.assumeIsolated { self.dragged() }
            },
            NSEvent.addGlobalMonitorForEvents(matching: .leftMouseUp) { _ in
                MainActor.assumeIsolated { self.released() }
            },
        ].compactMap { $0 }
    }

    /// Opens the wheel at the pointer for files picked some other way (the right-click menu).
    func open(for urls: [URL]) {
        guard !converter.isWorking, !urls.isEmpty else { return }
        open(for: urls, mode: .click, tools: false)
        // a click anywhere outside the wheel puts it away
        dismissal = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { _ in
            MainActor.assumeIsolated { self.close() }
        }
    }

    private func dragged() {
        let flags = NSEvent.modifierFlags
        if panel != nil {
            if model.mode == .drag {
                model.showingTools = flags.contains(.option)
            }
            return
        }
        guard flags.contains(.shift), !converter.isWorking else { return }
        let pasteboard = NSPasteboard(name: .drag)
        guard pasteboard.changeCount != seenDrag else { return }
        let urls = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        guard !urls.isEmpty else { return }
        seenDrag = pasteboard.changeCount
        open(for: urls, mode: .drag, tools: flags.contains(.option))
    }

    private func released() {
        guard panel != nil, model.mode == .drag else { return }
        // a drop on a bubble lands just before this, so give it a moment
        Task {
            do {
                try await Task.sleep(for: .milliseconds(150))
            } catch {
                return
            }
            close()
        }
    }

    private func open(for urls: [URL], mode: WheelModel.Mode, tools: Bool) {
        close()
        model.mode = mode
        model.urls = urls
        model.actions = []
        model.hovered = nil
        model.showingTools = tools

        let size: CGFloat = 470
        let view = WheelView(model: model, size: size) { action in
            self.converter.start(action, on: urls)
            self.close()
        }
        let panel = NSPanel.floating(FloatPanel.self, content: view)
        let mouse = NSEvent.mouseLocation
        panel.setFrame(NSRect(x: mouse.x - size / 2, y: mouse.y - size / 2, width: size, height: size), display: false)
        panel.hasShadow = false
        panel.orderFrontRegardless()
        self.panel = panel

        Task {
            let actions = await converter.actions(for: urls)
            guard model.urls == urls else { return } // a newer wheel took over
            model.actions = actions
        }
    }

    private func close() {
        if let dismissal {
            NSEvent.removeMonitor(dismissal)
        }
        dismissal = nil
        panel?.orderOut(nil)
        panel = nil
    }
}

struct WheelView: View {
    let model: WheelModel
    let size: CGFloat
    let pick: (String) -> Void

    var body: some View {
        let actions = model.visible
        let bubble: CGFloat = actions.count > 8 ? 74 : 84
        ZStack {
            hub
            ForEach(Array(actions.enumerated()), id: \.element.id) { index, action in
                Bubble(action: action, diameter: bubble, hovered: model.hovered == action.id) {
                    pick(action.id)
                } onHover: { inside in
                    if inside {
                        model.hovered = action.id
                    } else if model.hovered == action.id {
                        model.hovered = nil
                    }
                }
                .offset(offset(index, of: actions.count, bubble: bubble))
            }
        }
        .frame(width: size, height: size)
        .environment(\.colorScheme, .dark)
    }

    /// The middle: what's being converted. In click mode it flips between formats and tools.
    private var hub: some View {
        VStack(spacing: 2) {
            Text(model.urls.displayName).font(Ink.font(14)).lineLimit(1).truncationMode(.middle)
            Text(model.hint).font(Ink.font(11, bold: false)).foregroundStyle(Ink.faded).multilineTextAlignment(.center)
        }
        .foregroundStyle(Ink.pencil)
        .padding(.horizontal, 16)
        .frame(width: 146, height: 146)
        .background(Ink.paper, in: Circle())
        .overlay { SketchEllipse(seed: "hub").stroke(Ink.pencil, lineWidth: 1.3) }
        .contentShape(Circle())
        .onTapGesture {
            if model.mode == .click {
                model.showingTools.toggle()
            }
        }
    }

    /// Spread around the hub like the peacock's tail, starting at the top.
    private func offset(_ index: Int, of count: Int, bubble: CGFloat) -> CGSize {
        let angle = -Double.pi / 2 + Double(index) / Double(max(count, 1)) * 2 * .pi
        let radius = size / 2 - bubble / 2 - 6
        return CGSize(width: cos(angle) * radius, height: sin(angle) * radius)
    }
}

private struct Bubble: View {
    let action: Engine.Action
    let diameter: CGFloat
    let hovered: Bool
    let pick: () -> Void
    let onHover: (Bool) -> Void

    var body: some View {
        Text(action.label)
            .font(Ink.font(action.label.count > 10 ? 11 : 13))
            .multilineTextAlignment(.center)
            .lineLimit(2)
            .minimumScaleFactor(0.6)
            .padding(5)
            .foregroundStyle(hovered ? Ink.paper : Ink.pencil)
            .frame(width: diameter, height: diameter)
            .background {
                ZStack {
                    Circle().fill(hovered ? Ink.pencil : Ink.paper)
                    SketchEllipse(seed: action.id).stroke(Ink.pencil, lineWidth: 1.3)
                }
            }
            .scaleEffect(hovered ? 1.08 : 1)
            .animation(.easeOut(duration: 0.12), value: hovered)
            .contentShape(Circle())
            .onTapGesture(perform: pick)
            .onHover(perform: onHover)
            .dropDestination(for: URL.self) { _, _ in
                pick()
                return true
            } isTargeted: { onHover($0) }
    }
}
