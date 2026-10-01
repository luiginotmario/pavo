import Foundation

/// The `pavo` command-line tool bundled inside the app. The rust engine only runs while a
/// conversion does; the rest of the time pavo is just the menu bar app.
struct Engine {
    struct Action: Decodable {
        let id: String
        let label: String
        let group: String

        var isConversion: Bool { group == "convert" }
    }

    private let cli = if let path = ProcessInfo.processInfo.environment["PAVO_CLI"] {
        URL(fileURLWithPath: path)
    } else {
        Bundle.main.bundleURL.appending(path: "Contents/Helpers/pavo")
    }

    /// What the menu should offer for these files.
    func actions(for urls: [URL]) async throws -> [Action] {
        let output = Pipe()
        let process = Process()
        process.executableURL = cli
        process.arguments = ["actions", "--json"] + urls.map(\.path)
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        try process.run()

        var data = Data()
        for try await byte in output.fileHandleForReading.bytes {
            data.append(byte)
        }
        struct Response: Decodable { let actions: [Action] }
        return try JSONDecoder().decode(Response.self, from: data).actions
    }

    /// Starts converting. The returned conversion reports its progress until it's done.
    func convert(_ urls: [URL], with action: String) throws -> Conversion {
        try Conversion(cli: cli, action: action, urls: urls)
    }
}

extension [URL] {
    /// "clip.mov", or "3 files".
    var displayName: String {
        count == 1 ? self[0].lastPathComponent : "\(count) files"
    }
}
