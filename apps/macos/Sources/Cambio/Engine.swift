import Foundation

/// Runs the `cambio` command-line tool that ships inside the app. The rust engine only exists
/// while a conversion does; the rest of the time cambio is just this small menu bar app.
final class Engine {
    struct Action: Decodable {
        let id: String
        let label: String
        let group: String
    }

    enum Event {
        case progress(Double)
        case output(URL)
        case finished(error: String?)
    }

    /// A running conversion.
    final class Job {
        let name: String
        fileprivate(set) var progress: Double = 0
        fileprivate let process = Process()
        fileprivate let stdin = Pipe()

        fileprivate init(name: String) {
            self.name = name
        }

        /// The cli cancels itself when its stdin closes.
        func cancel() {
            try? stdin.fileHandleForWriting.close()
        }
    }

    private let cli: URL = {
        if let path = ProcessInfo.processInfo.environment["CAMBIO_CLI"] {
            return URL(fileURLWithPath: path)
        }
        return Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/cambio")
    }()

    func actions(for urls: [URL]) async throws -> [Action] {
        let process = Process()
        process.executableURL = cli
        process.arguments = ["actions", "--json"] + urls.map(\.path)
        let out = Pipe()
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        try process.run()

        let data = await Task.detached {
            let data = out.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            return data
        }.value

        struct Response: Decodable { let actions: [Action] }
        return try JSONDecoder().decode(Response.self, from: data).actions
    }

    @MainActor
    func run(_ action: String, on urls: [URL], events: @escaping @MainActor (Event) -> Void) throws -> Job {
        let job = Job(name: urls.count == 1 ? urls[0].lastPathComponent : "\(urls.count) files")
        let process = job.process
        process.executableURL = cli
        process.arguments = ["run", action, "--json", "--watch-stdin"] + urls.map(\.path)
        process.standardInput = job.stdin
        let out = Pipe()
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        // utility QoS keeps the work on apple silicon's efficiency cores
        process.qualityOfService = .utility
        try process.run()

        Task.detached(priority: .utility) {
            var error: String?
            do {
                for try await line in out.fileHandleForReading.bytes.lines {
                    guard let object = try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any],
                          let event = object["event"] as? String
                    else { continue }
                    switch event {
                    case "progress":
                        if let fraction = object["fraction"] as? Double {
                            await MainActor.run {
                                job.progress = fraction
                                events(.progress(fraction))
                            }
                        }
                    case "output":
                        if let path = object["path"] as? String {
                            await events(.output(URL(fileURLWithPath: path)))
                        }
                    case "error":
                        error = object["message"] as? String
                    default:
                        break
                    }
                }
            } catch {
                // the pipe broke; the exit status below says what happened
            }
            process.waitUntilExit()
            let failure = process.terminationStatus == 0 ? nil : (error ?? "something went wrong")
            await events(.finished(error: failure))
        }
        return job
    }
}
