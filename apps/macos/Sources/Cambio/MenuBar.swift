import AppKit
import os

private let log = Logger(subsystem: "com.giginotmario.cambio", category: "conversion")

/// The whole app: a peacock in the menu bar. Drop files on it (or click it and choose some),
/// pick what to turn them into, and the results land next to the originals.
final class MenuBar: NSObject, NSWindowDelegate, NSDraggingDestination {
    private enum State {
        case idle
        case converting(Conversion)
        case done([URL])
        case failed(String)
    }

    /// What a menu item hands back when it's picked.
    private struct Pick {
        let action: String
        let urls: [URL]
    }

    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    private let engine = Engine()
    private var state = State.idle
    private var badgeReset: Task<Void, Never>?

    private var isConverting: Bool {
        if case .converting = state { true } else { false }
    }

    override init() {
        super.init()
        guard let button = item.button else { return }
        button.image = PeacockIcon.make()
        button.imagePosition = .imageLeading
        button.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .medium)
        button.target = self
        button.action = #selector(clicked)
        // the button lives in its own little window; registering it is what makes the icon a drop target
        button.window?.registerForDraggedTypes([.fileURL])
        button.window?.delegate = self
    }

    // MARK: dropping files on the icon

    func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard !isConverting, !fileURLs(in: sender).isEmpty else { return [] }
        item.button?.highlight(true)
        return .copy
    }

    func draggingExited(_ sender: NSDraggingInfo?) {
        item.button?.highlight(false)
    }

    func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        item.button?.highlight(false)
        let urls = fileURLs(in: sender)
        guard !isConverting, !urls.isEmpty else { return false }
        // runs after the drag has finished, so the menu can take over the mouse
        Task { await offerActions(for: urls) }
        return true
    }

    private func fileURLs(in info: NSDraggingInfo) -> [URL] {
        info.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
    }

    // MARK: menus

    @objc private func clicked() {
        let menu = NSMenu()
        menu.autoenablesItems = false

        switch state {
        case .converting(let conversion):
            menu.addItem(.label("converting \(conversion.name)… \(percent(conversion.progress))"))
            menu.addItem(entry("cancel", #selector(cancel)))
        case .done(let urls):
            menu.addItem(entry("show \(urls.displayName) in finder", #selector(reveal)))
            menu.addItem(.separator())
            addStartItems(to: menu)
        case .failed(let message):
            menu.addItem(.label("couldn't do that: \(message.prefix(80))"))
            menu.addItem(.separator())
            addStartItems(to: menu)
            // once it's been read, the error is done
            state = .idle
            setBadge(nil)
        case .idle:
            addStartItems(to: menu)
        }

        menu.addItem(.separator())
        menu.addItem(entry("quit cambio", #selector(quit), key: "q"))
        show(menu)
    }

    private func addStartItems(to menu: NSMenu) {
        menu.addItem(.label("drop a file on the peacock to convert it"))
        menu.addItem(entry("choose files…", #selector(chooseFiles)))
    }

    private func offerActions(for urls: [URL]) async {
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.addItem(.label(urls.displayName))

        do {
            let actions = try await engine.actions(for: urls)
            let conversions = actions.filter(\.isConversion)
            let tools = actions.filter { !$0.isConversion }
            if !conversions.isEmpty {
                menu.addItem(.separator())
                menu.addItem(.label("convert to"))
                for action in conversions {
                    menu.addItem(entry(for: action, urls))
                }
            }
            if !tools.isEmpty {
                menu.addItem(.separator())
                for action in tools {
                    menu.addItem(entry(for: action, urls))
                }
            }
            if actions.isEmpty {
                menu.addItem(.label("nothing to do with this yet"))
            }
        } catch {
            log.error("couldn't list actions: \(error.localizedDescription, privacy: .public)")
            menu.addItem(.label("couldn't read \(urls.count == 1 ? "that file" : "those files")"))
        }
        show(menu)
    }

    /// Pops a menu down from the icon, exactly where a click would put it.
    private func show(_ menu: NSMenu) {
        item.menu = menu
        item.button?.performClick(nil)
        item.menu = nil
    }

    private func entry(_ title: String, _ action: Selector, key: String = "") -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.target = self
        return item
    }

    private func entry(for action: Engine.Action, _ urls: [URL]) -> NSMenuItem {
        let item = entry(action.label, #selector(picked(_:)))
        item.representedObject = Pick(action: action.id, urls: urls)
        return item
    }

    // MARK: actions

    @objc private func picked(_ sender: NSMenuItem) {
        guard let pick = sender.representedObject as? Pick, !isConverting else { return }
        let conversion: Conversion
        do {
            conversion = try engine.convert(pick.urls, with: pick.action)
        } catch {
            log.error("couldn't start the cli: \(error.localizedDescription, privacy: .public)")
            finish(.failed("couldn't start the converter"))
            return
        }

        state = .converting(conversion)
        setBadge(percent(0))
        Task {
            do throws(Conversion.Failure) {
                let outputs = try await conversion.run { fraction in setBadge(percent(fraction)) }
                finish(outputs.isEmpty ? .idle : .done(outputs))
            } catch .cancelled {
                finish(.idle)
            } catch .failed(let message) {
                log.error("\(pick.action, privacy: .public) failed: \(message, privacy: .private)")
                finish(.failed(message))
            }
        }
    }

    private func finish(_ next: State) {
        state = next
        switch next {
        case .done: setBadge("✓", clearAfter: .seconds(2))
        case .failed: setBadge("!")
        case .idle, .converting: setBadge(nil)
        }
    }

    @objc private func cancel() {
        if case .converting(let conversion) = state {
            conversion.cancel()
        }
    }

    @objc private func reveal() {
        if case .done(let urls) = state {
            NSWorkspace.shared.activateFileViewerSelecting(urls)
        }
    }

    @objc private func chooseFiles() {
        if #available(macOS 14, *) {
            NSApp.activate()
        } else {
            NSApp.activate(ignoringOtherApps: true)
        }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = true
        panel.prompt = "Choose"
        guard panel.runModal() == .OK, !panel.urls.isEmpty else { return }
        let urls = panel.urls
        Task { await offerActions(for: urls) }
    }

    @objc private func quit() {
        cancel()
        NSApp.terminate(nil)
    }

    // MARK: the little text next to the icon

    private func percent(_ fraction: Double) -> String {
        "\(Int(fraction * 100))%"
    }

    private func setBadge(_ text: String?, clearAfter delay: Duration? = nil) {
        badgeReset?.cancel()
        guard let button = item.button else { return }
        button.title = text.map { " " + $0 } ?? ""
        item.length = text == nil ? NSStatusItem.squareLength : NSStatusItem.variableLength

        guard let delay else { return }
        badgeReset = Task {
            do {
                try await Task.sleep(for: delay)
            } catch {
                return // a newer badge replaced this one
            }
            setBadge(nil)
        }
    }
}

private extension NSMenuItem {
    /// A line of text in a menu that can't be clicked.
    static func label(_ title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        return item
    }
}
