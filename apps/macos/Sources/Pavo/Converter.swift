import Foundation
import Observation
import os

private let log = Logger(subsystem: "com.giginotmario.pavo", category: "conversion")

/// What pavo is doing right now. The menu bar icon, the panel and the wheel all show this one model.
@Observable
final class Converter {
    enum Phase {
        case empty
        case loading([URL])
        case choosing([URL], [Engine.Action])
        case working(Conversion)
        case done([URL])
        case failed(String)
    }

    private(set) var phase = Phase.empty
    private(set) var progress = 0.0
    /// When the last conversion finished, so the menu bar can flash a ✓ for a moment.
    private(set) var finishedAt: Date?

    private let engine = Engine()

    var isWorking: Bool {
        if case .working = phase { true } else { false }
    }

    /// Looks up what can be done with these files.
    func load(_ urls: [URL]) async {
        guard !isWorking, !urls.isEmpty else { return }
        phase = .loading(urls)
        do {
            let actions = try await engine.actions(for: urls)
            // another drop may have replaced these files while we were waiting
            guard case .loading(let current) = phase, current == urls else { return }
            phase = .choosing(urls, actions)
        } catch {
            log.error("couldn't list actions: \(error.localizedDescription, privacy: .public)")
            phase = .failed("couldn't read \(urls.count == 1 ? "that file" : "those files")")
        }
    }

    /// Looks up actions without touching what the panel shows (for the wheel).
    func actions(for urls: [URL]) async -> [Engine.Action] {
        (try? await engine.actions(for: urls)) ?? []
    }

    func start(_ action: String, on urls: [URL]) {
        guard !isWorking else { return }
        let conversion: Conversion
        do {
            conversion = try engine.convert(urls, with: action)
        } catch {
            log.error("couldn't start the cli: \(error.localizedDescription, privacy: .public)")
            phase = .failed("couldn't start the converter")
            return
        }

        progress = 0
        phase = .working(conversion)
        Task {
            do throws(Conversion.Failure) {
                let outputs = try await conversion.run { progress = $0 }
                finishedAt = .now
                phase = outputs.isEmpty ? .empty : .done(outputs)
            } catch .cancelled {
                phase = .empty
            } catch .failed(let message) {
                log.error("\(action, privacy: .public) failed: \(message, privacy: .private)")
                phase = .failed(message)
            }
        }
    }

    func cancel() {
        if case .working(let conversion) = phase {
            conversion.cancel()
        }
    }

    /// Back to "drop a file", unless something is still converting.
    func reset() {
        if !isWorking {
            phase = .empty
        }
    }
}
