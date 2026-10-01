import AppKit

// No window, no dock icon: cambio is just a peacock in the menu bar.
MainActor.assumeIsolated {
    let app = NSApplication.shared
    let menuBar = MenuBar()
    app.setActivationPolicy(.accessory)
    withExtendedLifetime(menuBar) {
        app.run()
    }
}
