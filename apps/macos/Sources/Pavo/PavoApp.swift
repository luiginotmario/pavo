import AppKit

/// No window, no dock icon: pavo is a peacock in the menu bar and nothing else.
@main
final class PavoApp: NSObject, NSApplicationDelegate {
    /// NSApplication only holds its delegate weakly, so the app keeps the strong reference.
    private static let shared = PavoApp()
    /// Sent between copies of pavo so a second launch shows the first one's panel instead.
    private static let showPanel = Notification.Name("com.giginotmario.pavo.show-panel")

    private var menuBar: MenuBar?

    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        app.delegate = shared
        app.run()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // only one peacock. if another copy is already running, the installed one wins: a copy
        // left running from the disk image gets told to quit, otherwise this one hands over and quits
        let others = NSRunningApplication.runningApplications(withBundleIdentifier: Bundle.main.bundleIdentifier ?? "")
            .filter { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier }
        if !others.isEmpty {
            let othersAreTemporary = others.allSatisfy { Bundle.isTemporaryCopy(path: $0.bundleURL?.path ?? "") }
            if othersAreTemporary && !Bundle.main.isTemporaryCopy {
                others.forEach { $0.terminate() }
            } else {
                DistributedNotificationCenter.default().postNotificationName(Self.showPanel, object: nil, deliverImmediately: true)
                NSApp.terminate(nil)
                return
            }
        }

        let menuBar = MenuBar()
        self.menuBar = menuBar
        DistributedNotificationCenter.default().addObserver(forName: Self.showPanel, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { menuBar.showPanel() }
        }
        // nothing else is built until it's needed: the panel only exists while it's open
    }

    /// Opening pavo again while it's running (Finder, Spotlight, the Dock) shows the panel.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        menuBar?.showPanel()
        return false
    }
}

extension Bundle {
    /// Running straight from the disk image, or from the temporary copy macOS makes of an app
    /// that's opened where it was downloaded ("app translocation").
    static func isTemporaryCopy(path: String) -> Bool {
        path.hasPrefix("/Volumes/") || path.contains("/AppTranslocation/")
    }

    var isTemporaryCopy: Bool {
        Self.isTemporaryCopy(path: bundlePath)
    }
}
