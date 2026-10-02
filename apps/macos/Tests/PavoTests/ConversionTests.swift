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

struct InstallLocationTests {
    @Test(arguments: [
        ("/Applications/Pavo.app", false),
        ("/Users/someone/Applications/Pavo.app", false),
        ("/Volumes/Pavo/Pavo.app", true),
        ("/private/var/folders/x1/abc/T/AppTranslocation/1234-5678/d/Pavo.app", true),
    ])
    func `knows when pavo runs from the disk image`(path: String, temporary: Bool) {
        #expect(Bundle.isTemporaryCopy(path: path) == temporary)
    }
}

struct WatermarkBoxTests {
    let size = CGSize(width: 300, height: 200)

    @Test func `a box dragged either way comes out the same`() {
        let forward = FramePicker.fraction(from: CGPoint(x: 30, y: 20), to: CGPoint(x: 150, y: 100), in: size)
        let backward = FramePicker.fraction(from: CGPoint(x: 150, y: 100), to: CGPoint(x: 30, y: 20), in: size)
        #expect(forward == backward)
        #expect(forward == CGRect(x: 0.1, y: 0.1, width: 0.4, height: 0.4))
    }

    @Test func `a drag past the edge stops at the edge`() {
        let box = FramePicker.fraction(from: CGPoint(x: 240, y: 150), to: CGPoint(x: 400, y: 300), in: size)
        #expect(box.maxX == 1 && box.maxY == 1)
    }
}
