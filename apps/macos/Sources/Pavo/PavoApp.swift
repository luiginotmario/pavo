import AppKit

/// No window, no dock icon: pavo is a peacock in the menu bar and nothing else.
@main
final class PavoApp: NSObject, NSApplicationDelegate {
    /// NSApplication only holds its delegate weakly, so the app keeps the strong reference.
    private static let shared = PavoApp()

    private var menuBar: MenuBar?

    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        app.delegate = shared
        app.run()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        menuBar = MenuBar()
    }
}
