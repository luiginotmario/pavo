import AppKit
import Security
import os

private let log = Logger(subsystem: "com.giginotmario.pavo", category: "update")

/// Keeps pavo up to date without asking and without running in the background.
///
/// Nothing happens on a timer: when the peacock is clicked (at most once an hour) pavo asks GitHub for
/// the latest release. A newer one is downloaded, checked to be signed by pavo's own developer ID,
/// and staged next to the app. Once the panel is closed and nothing is converting, the two are
/// swapped with renames and pavo reopens. Any failure leaves the current version untouched.
final class Updater {
    private static let teamID = "LLW9KQB3R4"
    private static let lastCheckKey = "lastUpdateCheck"

    /// GitHub's latest release, or a stand-in feed for testing (PAVO_UPDATE_FEED).
    private let feed: URL
    private let interval: TimeInterval = 60 * 60
    private var checking = false
    /// A verified newer Pavo.app, waiting next to the current one.
    private(set) var staged: URL?

    init() {
        let environment = ProcessInfo.processInfo.environment
        feed = environment["PAVO_UPDATE_FEED"].flatMap(URL.init(string:))
            ?? URL(string: "https://api.github.com/repos/luiginotmario/pavo/releases/latest")!
    }

    private var app: URL { Bundle.main.bundleURL }
    private var folder: URL { app.deletingLastPathComponent() }

    /// Only a copy that lives somewhere pavo may write to, like Applications, updates itself.
    private var canUpdateInPlace: Bool {
        !Bundle.main.isTemporaryCopy && FileManager.default.isWritableFile(atPath: folder.path)
    }

    /// Called when the peacock is clicked. Does nothing unless it's been a while since the last look.
    func checkIfDue(onReady: @escaping () -> Void) {
        guard !checking, staged == nil, canUpdateInPlace else { return }
        let last = UserDefaults.standard.double(forKey: Self.lastCheckKey)
        guard Date.now.timeIntervalSince1970 - last > interval else { return }
        UserDefaults.standard.set(Date.now.timeIntervalSince1970, forKey: Self.lastCheckKey)

        checking = true
        Task {
            defer { checking = false }
            do {
                staged = try await stageNewerVersion()
                if staged != nil {
                    onReady()
                }
            } catch {
                log.error("update skipped: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    /// Swaps in the staged version and reopens pavo. Only call when the panel is closed and nothing converts.
    func install() {
        guard let staged else { return }
        let old = folder.appending(path: ".Pavo-old.app")
        let files = FileManager.default
        do {
            try? files.removeItem(at: old)
            try files.moveItem(at: app, to: old)
            do {
                try files.moveItem(at: staged, to: app)
            } catch {
                try? files.moveItem(at: old, to: app) // put the working version back
                throw error
            }
        } catch {
            log.error("couldn't swap in the update: \(error.localizedDescription, privacy: .public)")
            self.staged = nil
            return
        }

        // reopen once this copy has quit (a second copy would hand over to this one instead)
        let reopen = Process()
        reopen.executableURL = URL(fileURLWithPath: "/bin/sh")
        reopen.arguments = ["-c", "while kill -0 \(ProcessInfo.processInfo.processIdentifier) 2>/dev/null; do sleep 0.2; done; /usr/bin/open \"$0\"", app.path]
        do {
            try reopen.run()
        } catch {
            log.error("couldn't reopen after updating: \(error.localizedDescription, privacy: .public)")
        }
        NSApp.terminate(nil)
    }

    /// Leftovers from the last update, cleared on launch.
    func tidyUp() {
        for name in [".Pavo-old.app", ".Pavo-update.app"] {
            try? FileManager.default.removeItem(at: folder.appending(path: name))
        }
    }

    // MARK: fetching

    private struct Release: Decodable {
        struct Asset: Decodable {
            let name: String
            let browser_download_url: URL
        }
        let tag_name: String
        let assets: [Asset]
    }

    private enum Problem: Error {
        case noDownload, unsigned, wrongVersion, tool(String)
    }

    private func stageNewerVersion() async throws -> URL? {
        let (data, _) = try await URLSession.shared.data(from: feed)
        let release = try JSONDecoder().decode(Release.self, from: data)
        let current = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
        guard Self.isNewer(release.tag_name, than: current) else { return nil }
        guard let dmgURL = release.assets.first(where: { $0.name == "Pavo.dmg" })?.browser_download_url else {
            throw Problem.noDownload
        }

        let work = FileManager.default.temporaryDirectory.appending(path: "pavo-update-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: work) }

        let (downloaded, _) = try await URLSession.shared.download(from: dmgURL)
        let dmg = work.appending(path: "Pavo.dmg")
        try FileManager.default.moveItem(at: downloaded, to: dmg)

        let mount = work.appending(path: "mount")
        try await Self.run("/usr/bin/hdiutil", ["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint", mount.path, dmg.path])
        let staged: Result<URL, Error>
        do {
            staged = .success(try await stage(mount.appending(path: "Pavo.app"), newerThan: current))
        } catch {
            staged = .failure(error)
        }
        // the signature check can keep files on the image open, so a polite detach isn't enough
        try? await Self.run("/usr/bin/hdiutil", ["detach", "-force", "-quiet", mount.path])
        let app = try staged.get()
        log.notice("staged \(release.tag_name, privacy: .public)")
        return app
    }

    /// Checks the new app is pavo's and newer, then copies it next to the current one.
    private func stage(_ newApp: URL, newerThan current: String) async throws -> URL {
        guard Self.isSignedByUs(newApp) else { throw Problem.unsigned }
        let newVersion = Bundle(url: newApp)?.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        guard newVersion.map({ Self.isNewer($0, than: current) }) == true else { throw Problem.wrongVersion }

        let staged = folder.appending(path: ".Pavo-update.app")
        try? FileManager.default.removeItem(at: staged)
        try await Self.run("/usr/bin/ditto", [newApp.path, staged.path])
        guard Self.isSignedByUs(staged) else {
            try? FileManager.default.removeItem(at: staged)
            throw Problem.unsigned
        }
        return staged
    }

    // MARK: checks

    /// "v0.1.10" is newer than "0.1.9".
    static func isNewer(_ candidate: String, than current: String) -> Bool {
        let parts = { (version: String) in
            version.trimmingCharacters(in: CharacterSet(charactersIn: "v")).split(separator: ".").map { Int($0) ?? 0 }
        }
        let (a, b) = (parts(candidate), parts(current))
        for i in 0..<max(a.count, b.count) {
            let (x, y) = (i < a.count ? a[i] : 0, i < b.count ? b[i] : 0)
            if x != y { return x > y }
        }
        return false
    }

    /// Valid, untampered, and signed with pavo's developer ID: nobody else can ship an update.
    static func isSignedByUs(_ app: URL) -> Bool {
        var code: SecStaticCode?
        var requirement: SecRequirement?
        let rule = "anchor apple generic and certificate leaf[subject.OU] = \"\(teamID)\"" as CFString
        guard SecStaticCodeCreateWithPath(app as CFURL, [], &code) == errSecSuccess, let code,
              SecRequirementCreateWithString(rule, [], &requirement) == errSecSuccess
        else { return false }
        let flags = SecCSFlags(rawValue: kSecCSCheckAllArchitectures | kSecCSStrictValidate | kSecCSCheckNestedCode)
        return SecStaticCodeCheckValidity(code, flags, requirement) == errSecSuccess
    }

    private static func run(_ tool: String, _ arguments: [String]) async throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: tool)
        process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        let (exit, exited) = AsyncStream.makeStream(of: Int32.self)
        process.terminationHandler = { finished in
            exited.yield(finished.terminationStatus)
            exited.finish()
        }
        try process.run()
        var status: Int32 = -1
        for await code in exit {
            status = code
        }
        guard status == 0 else { throw Problem.tool("\(tool) exited with \(status)") }
    }
}
