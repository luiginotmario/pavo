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
        // only one peacock: if pavo is already running (say, one copy in Applications and one
        // opened from the disk image), hand over to it and bow out
        let others = NSRunningApplication.runningApplications(withBundleIdentifier: Bundle.main.bundleIdentifier ?? "")
            .filter { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier }
        if !others.isEmpty {
            DistributedNotificationCenter.default().postNotificationName(Self.showPanel, object: nil, deliverImmediately: true)
            NSApp.terminate(nil)
            return
        }

        let menuBar = MenuBar()
        self.menuBar = menuBar
        DistributedNotificationCenter.default().addObserver(forName: Self.showPanel, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { menuBar.showPanel() }
        }
        // there's no window, so show where pavo lives the moment it's opened
        menuBar.showPanel()
    }

    /// Opening pavo again while it's running (Finder, Spotlight, the Dock) shows the panel.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        menuBar?.showPanel()
        return false
    }
}
