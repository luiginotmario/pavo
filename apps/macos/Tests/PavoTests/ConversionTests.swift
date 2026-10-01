import Foundation
import Testing

@testable import Pavo

/// The app and the rust cli talk over one json object per line. These pin down that contract.
struct CLIOutputTests {
    @Test func `reads progress`() {
        #expect(Conversion.Update(line: #"{"event":"progress","fraction":0.42}"#) == .progress(0.42))
    }

    @Test func `reads finished files`() {
        let update = Conversion.Update(line: #"{"event":"output","path":"/tmp/clip.mp4"}"#)
        #expect(update == .output(URL(fileURLWithPath: "/tmp/clip.mp4")))
    }

    @Test func `reads errors`() {
        let update = Conversion.Update(line: #"{"event":"error","message":"clip.mov has no sound in it"}"#)
        #expect(update == .failed("clip.mov has no sound in it"))
    }

    @Test(arguments: [
        "",
        "not json",
        #"{"event":"start","file":"/tmp/a.mov","index":0,"total":1}"#,
        #"{"event":"done","outputs":[]}"#,
        #"{"event":"progress"}"#,
    ])
    func `ignores everything else`(line: String) {
        #expect(Conversion.Update(line: line) == nil)
    }

    @Test(arguments: [(["/a/clip.mov"], "clip.mov"), (["/a/1.png", "/a/2.png", "/a/3.png"], "3 files")])
    func `names what was dropped`(paths: [String], name: String) {
        #expect(paths.map { URL(fileURLWithPath: $0) }.displayName == name)
    }
}

nonisolated private let builtCLI = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()
    .appending(path: "../../../../target/release/pavo")
    .standardized

/// Runs the real rust cli through `Conversion`, the same way the menu bar does.
/// Needs `cargo build --release` first; skipped otherwise.
@Suite(.enabled(if: FileManager.default.isExecutableFile(atPath: builtCLI.path)))
struct EndToEndTests {

    let folder = FileManager.default.temporaryDirectory.appending(path: "pavo-swift-\(UUID().uuidString)")
    let png: URL

    init() throws {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        png = folder.appending(path: "dot.png")
        let onePixel = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
        try #require(Data(base64Encoded: onePixel)).write(to: png)
    }

    @Test func `converts a file and reports where it went`() async throws {
        let conversion = try Conversion(cli: builtCLI, action: "to:jpg", urls: [png])
        var reported: [Double] = []
        let outputs = try await conversion.run { reported.append($0) }

        #expect(outputs == [folder.appending(path: "dot.jpg")])
        #expect(FileManager.default.fileExists(atPath: outputs[0].path))
        #expect(reported.last == 1)
    }

    @Test func `passes the cli's error through`() async throws {
        let conversion = try Conversion(cli: builtCLI, action: "pdf:split", urls: [png])
        await #expect(throws: Conversion.Failure.failed("can't pdf:split dot.png")) {
            try await conversion.run { _ in }
        }
    }
}
