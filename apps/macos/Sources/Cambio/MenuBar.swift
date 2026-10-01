import AppKit

/// The whole app: a peacock in the menu bar. Drop files on it (or click it and choose some),
/// pick what to turn them into, and the results land next to the originals.
@MainActor
final class MenuBar: NSObject, NSWindowDelegate, NSDraggingDestination {
    private enum Outcome {
        case done([URL])
        case failed(String)
    }

    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    private let engine = Engine()
    private var job: Engine.Job?
    private var outputs: [URL] = []
    private var outcome: Outcome?
    private var clearBadge: DispatchWorkItem?

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
        guard job == nil, !fileURLs(in: sender).isEmpty else { return [] }
        item.button?.highlight(true)
        return .copy
    }

    func draggingExited(_ sender: NSDraggingInfo?) {
        item.button?.highlight(false)
    }

    func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        item.button?.highlight(false)
        let urls = fileURLs(in: sender)
        guard job == nil, !urls.isEmpty else { return false }
        // let the drag finish before a menu takes over the mouse
        DispatchQueue.main.async { self.offerActions(for: urls) }
        return true
    }

    private func fileURLs(in info: NSDraggingInfo) -> [URL] {
        info.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
    }

    // MARK: menus

    @objc private func clicked() {
        let menu = NSMenu()
        menu.autoenablesItems = false

        if let job {
            menu.addItem(label("converting \(job.name)… \(Int(job.progress * 100))%"))
            menu.addItem(entry("cancel", #selector(cancel)))
        } else {
            switch outcome {
            case .done(let urls)?:
                let name = urls.count == 1 ? urls[0].lastPathComponent : "\(urls.count) files"
                menu.addItem(entry("show \(name) in finder", #selector(reveal)))
                menu.addItem(.separator())
            case .failed(let message)?:
                menu.addItem(label("couldn't do that: \(message.prefix(80))"))
                menu.addItem(.separator())
                outcome = nil
                setBadge(nil)
            case nil:
                break
            }
            menu.addItem(label("drop a file on the peacock to convert it"))
            menu.addItem(entry("choose files…", #selector(chooseFiles)))
        }

        menu.addItem(.separator())
        menu.addItem(entry("quit cambio", #selector(quit), key: "q"))
        show(menu)
    }

    private func offerActions(for urls: [URL]) {
        Task {
            let actions = (try? await engine.actions(for: urls)) ?? []
            let menu = NSMenu()
            menu.autoenablesItems = false
            menu.addItem(label(urls.count == 1 ? urls[0].lastPathComponent : "\(urls.count) files"))

            let converts = actions.filter { $0.group == "convert" }
            let tools = actions.filter { $0.group != "convert" }
            if !converts.isEmpty {
                menu.addItem(.separator())
                menu.addItem(label("convert to"))
                converts.forEach { menu.addItem(actionEntry($0, urls)) }
            }
            if !tools.isEmpty {
                menu.addItem(.separator())
                tools.forEach { menu.addItem(actionEntry($0, urls)) }
            }
            if actions.isEmpty {
                menu.addItem(label("nothing to do with this yet"))
            }
            show(menu)
        }
    }

    /// Pops a menu down from the icon, the same way a click would.
    private func show(_ menu: NSMenu) {
        item.menu = menu
        item.button?.performClick(nil)
        item.menu = nil
    }

    private func label(_ title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        return item
    }

    private func entry(_ title: String, _ action: Selector, key: String = "") -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.target = self
        return item
    }

    private final class Request {
        let action: String
        let urls: [URL]
        init(action: String, urls: [URL]) {
            self.action = action
            self.urls = urls
        }
    }

    private func actionEntry(_ action: Engine.Action, _ urls: [URL]) -> NSMenuItem {
        let item = entry(action.label, #selector(picked(_:)))
        item.representedObject = Request(action: action.id, urls: urls)
        return item
    }

    // MARK: actions

    @objc private func picked(_ sender: NSMenuItem) {
        guard let request = sender.representedObject as? Request, job == nil else { return }
        outputs = []
        outcome = nil
        do {
            job = try engine.run(request.action, on: request.urls) { [weak self] event in
                self?.handle(event)
            }
            setBadge("0%")
        } catch {
            finish(error: "couldn't start the converter")
        }
    }

    private func handle(_ event: Engine.Event) {
        switch event {
        case .progress(let fraction):
            setBadge("\(Int(fraction * 100))%")
        case .output(let url):
            outputs.append(url)
        case .finished(let error):
            finish(error: error)
        }
    }

    private func finish(error: String?) {
        job = nil
        if let error, error != "cancelled" {
            outcome = .failed(error)
            setBadge("!")
        } else if !outputs.isEmpty {
            outcome = .done(outputs)
            setBadge("✓", clearAfter: 2)
        } else {
            setBadge(nil)
        }
    }

    @objc private func cancel() {
        job?.cancel()
    }

    @objc private func reveal() {
        if case .done(let urls)? = outcome {
            NSWorkspace.shared.activateFileViewerSelecting(urls)
        }
    }

    @objc private func chooseFiles() {
        NSApp.activate(ignoringOtherApps: true)
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = true
        panel.prompt = "Choose"
        guard panel.runModal() == .OK, !panel.urls.isEmpty else { return }
        offerActions(for: panel.urls)
    }

    @objc private func quit() {
        job?.cancel()
        NSApp.terminate(nil)
    }

    // MARK: the little text next to the icon

    private func setBadge(_ text: String?, clearAfter seconds: Double? = nil) {
        clearBadge?.cancel()
        guard let button = item.button else { return }
        if let text {
            button.title = " " + text
            item.length = NSStatusItem.variableLength
        } else {
            button.title = ""
            item.length = NSStatusItem.squareLength
        }
        if let seconds {
            let work = DispatchWorkItem { [weak self] in self?.setBadge(nil) }
            clearBadge = work
            DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: work)
        }
    }
}
