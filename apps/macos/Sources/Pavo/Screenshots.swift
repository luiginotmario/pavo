import AppKit
import Observation
import QuickLookThumbnailing
import UniformTypeIdentifiers
import os

private let log = Logger(subsystem: "com.giginotmario.pavo", category: "screenshots")

/// New screenshots go straight onto the clipboard, ready for ⌘V wherever you are. When macOS saves
/// them to the Desktop, they're also filed into Pictures/Screenshots/<year-month> so the Desktop
/// doesn't fill up. Nothing polls: the kernel says when the screenshot folder changes.
///
/// The last three stay in the panel to drag into any app. Once one is used it leaves the row.
/// Only their paths are kept, never the pictures, and they're forgotten when pavo quits.
@Observable
final class Screenshots {
    private static let onKey = "screenshotsToClipboard"

    private(set) var isOn = UserDefaults.standard.object(forKey: onKey) as? Bool ?? true
    /// Newest first.
    private(set) var recent: [URL] = []

    @ObservationIgnored private var source: DispatchSourceFileSystemObject?
    @ObservationIgnored private var folder: URL?
    /// Screenshots already handled, so a folder change that isn't a new one does nothing.
    @ObservationIgnored private var handled: Set<String> = []

    func turn(on: Bool) {
        UserDefaults.standard.set(on, forKey: Self.onKey)
        isOn = on
        if on {
            start()
        } else {
            stop()
            recent = []
        }
    }

    func start() {
        stop()
        let folder = Self.screenshotFolder()
        let descriptor = open(folder.path, O_EVTONLY)
        guard descriptor >= 0 else {
            log.error("can't watch \(folder.path, privacy: .private)")
            return
        }
        self.folder = folder
        handled = Set(Self.screenshots(in: folder, since: .distantPast).map(\.lastPathComponent))

        let source = DispatchSource.makeFileSystemObjectSource(fileDescriptor: descriptor, eventMask: .write, queue: .main)
        // the handler holds this object until stop() cancels the source
        source.setEventHandler {
            MainActor.assumeIsolated { self.folderChanged() }
        }
        source.setCancelHandler { close(descriptor) }
        source.resume()
        self.source = source
    }

    func stop() {
        source?.cancel()
        source = nil
    }

    private func folderChanged() {
        guard let folder else { return }
        for screenshot in Self.screenshots(in: folder, since: .now.addingTimeInterval(-30)) where handled.insert(screenshot.lastPathComponent).inserted {
            let filed = Self.isDesktop(folder) ? Self.file(screenshot) : screenshot
            Self.copyToClipboard(filed)
            remember(filed)
        }
    }

    func remember(_ screenshot: URL) {
        recent = Array(([screenshot] + recent).prefix(3))
    }

    func forget(_ screenshot: URL) {
        recent.removeAll { $0 == screenshot }
    }

    /// What dragging a screenshot out of the panel carries: the file itself. It leaves the row
    /// once another app takes it, so a drag that's called off keeps it.
    func dragItem(for screenshot: URL) -> NSItemProvider {
        let provider = NSItemProvider()
        provider.suggestedName = screenshot.deletingPathExtension().lastPathComponent
        let type = UTType(filenameExtension: screenshot.pathExtension) ?? .image
        provider.registerFileRepresentation(forTypeIdentifier: type.identifier, fileOptions: [], visibility: .all) { send in
            send(screenshot, false, nil)
            Task { @MainActor in self.forget(screenshot) }
            return nil
        }
        provider.registerItem(forTypeIdentifier: UTType.fileURL.identifier) { send, _, _ in
            send?(screenshot as NSURL, nil)
            Task { @MainActor in self.forget(screenshot) }
        }
        return provider
    }

    // MARK: finding them

    /// Where macOS saves screenshots: the Desktop unless someone picked a folder in the Screenshot app.
    static func screenshotFolder() -> URL {
        let custom = UserDefaults(suiteName: "com.apple.screencapture")?.string(forKey: "location")
        let path = (custom.map { NSString(string: $0).expandingTildeInPath }) ?? NSHomeDirectory() + "/Desktop"
        return URL(fileURLWithPath: path, isDirectory: true)
    }

    static func isDesktop(_ folder: URL) -> Bool {
        folder.standardizedFileURL.path == URL(fileURLWithPath: NSHomeDirectory() + "/Desktop").standardizedFileURL.path
    }

    /// Image files macOS marked as screenshots, made after `since`. Recordings and everything else are left alone.
    static func screenshots(in folder: URL, since: Date) -> [URL] {
        let files = (try? FileManager.default.contentsOfDirectory(
            at: folder, includingPropertiesForKeys: [.creationDateKey], options: [.skipsHiddenFiles]
        )) ?? []
        return files.filter { url in
            guard ["png", "jpg", "jpeg", "heic", "tiff"].contains(url.pathExtension.lowercased()),
                  let created = try? url.resourceValues(forKeys: [.creationDateKey]).creationDate,
                  created >= since
            else { return false }
            return getxattr(url.path, "com.apple.metadata:kMDItemIsScreenCapture", nil, 0, 0, 0) >= 0
        }
    }

    // MARK: handling one

    /// Moves a Desktop screenshot into Pictures/Screenshots/<year-month>. Never overwrites.
    static func file(_ screenshot: URL) -> URL {
        let month = Date.now.formatted(.iso8601.year().month())
        let folder = URL(fileURLWithPath: NSHomeDirectory()).appending(path: "Pictures/Screenshots/\(month)")
        var destination = folder.appending(path: screenshot.lastPathComponent)
        var copy = 2
        while FileManager.default.fileExists(atPath: destination.path) {
            destination = folder.appending(path: "\(screenshot.deletingPathExtension().lastPathComponent) \(copy).\(screenshot.pathExtension)")
            copy += 1
        }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try FileManager.default.moveItem(at: screenshot, to: destination)
            return destination
        } catch {
            log.error("couldn't file a screenshot: \(error.localizedDescription, privacy: .public)")
            return screenshot // still copied to the clipboard from where it is
        }
    }

    /// The picture for apps that take images, and the file for places that take files.
    static func copyToClipboard(_ screenshot: URL) {
        let item = NSPasteboardItem()
        if screenshot.pathExtension.lowercased() == "png", let png = try? Data(contentsOf: screenshot) {
            item.setData(png, forType: .png)
        } else if let tiff = NSImage(contentsOf: screenshot)?.tiffRepresentation {
            item.setData(tiff, forType: .tiff)
        }
        item.setString(screenshot.absoluteString, forType: .fileURL)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.writeObjects([item])
    }

    /// A small picture of the screenshot. Quick Look draws it in its own process, so the
    /// full-size image never lands in pavo's memory.
    static func thumbnail(of screenshot: URL, size: CGSize) async -> CGImage? {
        let request = QLThumbnailGenerator.Request(fileAt: screenshot, size: size, scale: 2, representationTypes: .thumbnail)
        return try? await QLThumbnailGenerator.shared.generateBestRepresentation(for: request).cgImage
    }
}
