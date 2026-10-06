import AppKit
import Observation

/// The peacock in the menu bar. Click it, or drop files on it, and the panel opens underneath.
/// While something converts, the progress sits next to the icon.
final class MenuBar: NSObject, NSWindowDelegate, NSDraggingDestination {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    private let converter = Converter()
    private let wheel: Wheel
    private lazy var panel = MenuPanel(
        converter: converter,
        chooseFiles: { self.chooseFiles() },
        screenshots: screenshots,
        closed: { self.installIfIdle() }
    )
    private let updater = Updater()
    private let screenshots = Screenshots()
    private var badgeReset: Task<Void, Never>?


    override init() {
        wheel = Wheel(converter: converter)
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
        followConverter()
        updater.tidyUp()
        if screenshots.isOn {
            screenshots.start()
        }

        // right-click files → Convert with Pavo (the service is declared in Info.plist)
        NSApp.servicesProvider = self
        NSUpdateDynamicServices()
    }

    // MARK: right-click → Convert with Pavo

    @objc func convertFiles(_ pasteboard: NSPasteboard, userData: String?, error: AutoreleasingUnsafeMutablePointer<NSString?>) {
        let urls = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        wheel.open(for: urls)
    }

    // MARK: dropping files on the icon

    func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard !converter.isWorking, !fileURLs(in: sender).isEmpty else { return [] }
        item.button?.highlight(true)
        return .copy
    }

    func draggingExited(_ sender: NSDraggingInfo?) {
        item.button?.highlight(false)
    }

    func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        item.button?.highlight(false)
        let urls = fileURLs(in: sender)
        guard !converter.isWorking, !urls.isEmpty else { return false }
        openPanel()
        Task { await converter.load(urls) }
        return true
    }

    private func fileURLs(in info: NSDraggingInfo) -> [URL] {
        info.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
    }

    // MARK: the panel

    @objc private func clicked() {
        if panel.isOpen {
            panel.close()
        } else {
            openPanel()
        }
    }

    /// Drops the panel down from the peacock (when pavo is opened, or opened again).
    func showPanel() {
        // the status item needs a moment to land in the menu bar on a fresh launch
        Task {
            try? await Task.sleep(for: .milliseconds(150))
            openPanel()
        }
    }

    private func openPanel() {
        checkForUpdate()
        guard let button = item.button, let window = button.window else { return }
        panel.open(below: window.convertToScreen(button.convert(button.bounds, to: nil)))
    }

    // MARK: updates: only checked when the peacock is clicked, never on a timer

    private func checkForUpdate() {
        updater.checkIfDue { self.installIfIdle() }
    }

    /// Swaps in a downloaded update, but only once nothing is open or converting.
    private func installIfIdle() {
        guard updater.staged != nil, !panel.isOpen, !wheel.isOpen, !converter.isWorking else { return }
        updater.install()
    }

    private func chooseFiles() {
        panel.close()
        NSApp.activate()
        let open = NSOpenPanel()
        open.allowsMultipleSelection = true
        open.canChooseDirectories = true
        open.prompt = "Choose"
        guard open.runModal() == .OK, !open.urls.isEmpty else { return }
        let urls = open.urls
        openPanel()
        Task { await converter.load(urls) }
    }

    // MARK: the little text next to the icon

    /// Re-renders the badge whenever the converter changes.
    private func followConverter() {
        withObservationTracking {
            updateBadge()
        } onChange: {
            Task { @MainActor in self.followConverter() }
        }
    }

    private func updateBadge() {
        switch converter.phase {
        case .working:
            setBadge("\(Int(converter.progress * 100))%")
        case .failed:
            setBadge("!")
        case .done:
            let shownFor = converter.finishedAt.map { Date.now.timeIntervalSince($0) } ?? 99
            setBadge(shownFor < 2 ? "✓" : nil, clearAfter: .seconds(2 - shownFor))
        case .empty, .loading, .choosing:
            setBadge(nil)
        }
        if !converter.isWorking {
            Task { installIfIdle() } // a conversion just finished: a good moment
        }
    }

    private func setBadge(_ text: String?, clearAfter delay: Duration? = nil) {
        badgeReset?.cancel()
        guard let button = item.button else { return }
        button.title = text.map { " " + $0 } ?? ""
        item.length = text == nil ? NSStatusItem.squareLength : NSStatusItem.variableLength
        guard text != nil, let delay, delay > .zero else { return }
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
