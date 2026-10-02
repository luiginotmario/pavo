import AppKit
import SwiftUI

/// A borderless panel that can take the keyboard (Spotlight-style) without pulling pavo to the front.
final class KeyPanel: NSPanel {
    override var canBecomeKey: Bool { true }
}

/// A borderless panel that never takes focus: the wheel floats over Finder mid-drag.
final class FloatPanel: NSPanel {
    override var canBecomeKey: Bool { false }
}

extension NSPanel {
    static func floating<Panel: NSPanel>(_ type: Panel.Type, content: some View) -> Panel {
        let panel = Panel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .popUpMenu
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        panel.isReleasedWhenClosed = false
        // panels hide when their app isn't frontmost by default, and a menu bar app rarely is
        panel.hidesOnDeactivate = false
        let host = NSHostingView(rootView: content)
        host.sizingOptions = [.intrinsicContentSize]
        panel.contentView = host
        return panel
    }
}

/// The panel under the menu bar icon. It's only built while it's open, so a closed
/// pavo carries no SwiftUI views around.
final class MenuPanel {
    private let converter: Converter
    private let chooseFiles: () -> Void
    private var panel: KeyPanel?
    private var anchor = NSRect.zero
    private var monitors: [Any] = []

    var isOpen: Bool { panel != nil }

    init(converter: Converter, chooseFiles: @escaping () -> Void) {
        self.converter = converter
        self.chooseFiles = chooseFiles
    }

    /// Opens under `anchor` (the status item's frame, in screen coordinates).
    func open(below anchor: NSRect) {
        self.anchor = anchor
        if let panel {
            place(panel)
            return
        }
        // the closures below hold this object until close() lets go of the panel and the monitors
        let view = PanelView(
            converter: converter,
            chooseFiles: chooseFiles,
            close: { self.close() },
            quit: { NSApp.terminate(nil) },
            resized: { self.place(size: $0) }
        )
        let panel = NSPanel.floating(KeyPanel.self, content: view)
        self.panel = panel
        place(panel)
        panel.makeKeyAndOrderFront(nil)

        // a click anywhere else closes it, but pressing on a file to drag it in doesn't:
        // the panel only closes when the press ends without the mouse having moved
        monitors = [
            NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp, .rightMouseDown]) { event in
                MainActor.assumeIsolated { self.outsideMouse(event.type) }
            },
            NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
                guard event.keyCode == 53 else { return event } // esc
                self.close()
                return nil
            },
        ].compactMap { $0 }
    }

    private var clickingOutside = false

    private func outsideMouse(_ type: NSEvent.EventType) {
        switch type {
        case .leftMouseDown: clickingOutside = true
        case .leftMouseDragged: clickingOutside = false // a drag, maybe of a file into the panel
        case .leftMouseUp where clickingOutside: close()
        case .rightMouseDown: close()
        default: break
        }
    }

    func close() {
        monitors.forEach(NSEvent.removeMonitor)
        monitors = []
        clickingOutside = false
        panel?.orderOut(nil)
        panel = nil
        if case .done = converter.phase { converter.reset() }
        if case .failed = converter.phase { converter.reset() }
    }

    private func place(_ panel: NSPanel) {
        guard let content = panel.contentView else { return }
        place(size: content.fittingSize)
    }

    /// Keeps the top edge pinned under the icon as the content grows and shrinks.
    private func place(size: CGSize) {
        guard let panel, size.width > 0 else { return }
        let screen = NSScreen.screens.first { $0.frame.intersects(anchor) } ?? NSScreen.main
        let visible = screen?.visibleFrame ?? .zero
        var x = anchor.midX - size.width / 2
        x = min(max(x, visible.minX + 6), visible.maxX - size.width - 6)
        panel.setFrame(NSRect(x: x, y: anchor.minY - size.height - 2, width: size.width, height: size.height), display: true)
    }
}
