import Foundation

/// One `pavo run` in flight. Closing its stdin cancels it, and if the app goes away the pipe
/// closes on its own, so a conversion can never outlive pavo.
final class Conversion {
    enum Failure: Error, Equatable {
        case cancelled
        case failed(String)
    }

    /// One line of the cli's `--json` output.
    enum Update: Equatable {
        case progress(Double)
        case output(URL)
        case failed(String)

        init?(line: String) {
            struct Line: Decodable {
                let event: String
                let fraction: Double?
                let path: String?
                let message: String?
            }
            guard let line = try? JSONDecoder().decode(Line.self, from: Data(line.utf8)) else { return nil }
            switch (line.event, line.fraction, line.path) {
            case ("progress", let fraction?, _): self = .progress(fraction)
            case ("output", _, let path?): self = .output(URL(fileURLWithPath: path))
            case ("error", _, _): self = .failed(line.message ?? "something went wrong")
            default: return nil
            }
        }
    }

    let name: String
    private(set) var progress = 0.0

    private let process = Process()
    private let input = Pipe()
    private let output = Pipe()
    private let exitStatus: AsyncStream<Int32>

    init(cli: URL, action: String, urls: [URL]) throws {
        name = urls.displayName
        process.executableURL = cli
        process.arguments = ["run", action, "--json", "--watch-stdin"] + urls.map(\.path)
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        // utility QoS keeps the work on apple silicon's efficiency cores
        process.qualityOfService = .utility

        // a stream buffers the exit status, so it's there even if the process exits before anyone asks
        let (exitStatus, exited) = AsyncStream.makeStream(of: Int32.self)
        self.exitStatus = exitStatus
        process.terminationHandler = { process in
            exited.yield(process.terminationStatus)
            exited.finish()
        }
        try process.run()
    }

    func cancel() {
        // closing an already-closed pipe throws, and that's fine: it's cancelled either way
        try? input.fileHandleForWriting.close()
    }

    /// Reports progress until the cli exits, then returns everything it made.
    func run(onProgress: (Double) -> Void) async throws(Failure) -> [URL] {
        var outputs: [URL] = []
        var message: String?
        do {
            for try await line in output.fileHandleForReading.bytes.lines {
                switch Update(line: line) {
                case .progress(let fraction)?:
                    progress = fraction
                    onProgress(fraction)
                case .output(let url)?:
                    outputs.append(url)
                case .failed(let why)?:
                    message = why
                case nil:
                    continue
                }
            }
        } catch {
            message = message ?? "lost touch with the converter"
        }

        var status: Int32 = -1
        for await code in exitStatus {
            status = code
        }
        guard status == 0 else {
            throw message == "cancelled" ? .cancelled : .failed(message ?? "something went wrong")
        }
        return outputs
    }
}
