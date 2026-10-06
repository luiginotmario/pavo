import AppKit
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

struct UpdaterTests {
    @Test(arguments: [
        ("v0.1.6", "0.1.5", true),
        ("0.1.10", "0.1.9", true),
        ("v0.2.0", "0.1.99", true),
        ("v0.1.5", "0.1.5", false),
        ("v0.1.4", "0.1.5", false),
        ("v1.0", "0.9.9", true),
    ])
    func `only installs versions that are actually newer`(candidate: String, current: String, newer: Bool) {
        #expect(Updater.isNewer(candidate, than: current) == newer)
    }

    @Test func `only trusts apps signed with pavo's developer id`() throws {
        let ours = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../../../build/Pavo.app").standardized
        try #require(FileManager.default.fileExists(atPath: ours.path), "run scripts/build.sh first")
        let developerID = Updater.isSignedByUs(ours)
        // a local build without the developer id certificate is only ad-hoc signed
        #expect(developerID == ProcessInfo.processInfo.environment["PAVO_SIGNED"].map { $0 == "1" } ?? developerID)
        #expect(!Updater.isSignedByUs(URL(fileURLWithPath: "/System/Applications/Calculator.app")), "apple's own apps aren't ours")
    }
}

struct DragPauseTests {
    let start = CGPoint(x: 400, y: 300)

    @Test func `a pause near the file opens the wheel`() {
        #expect(Wheel.isWondering(start: start, at: CGPoint(x: 460, y: 340), stillFor: .milliseconds(700)))
    }

    @Test func `moving along never opens it`() {
        #expect(!Wheel.isWondering(start: start, at: CGPoint(x: 420, y: 310), stillFor: .milliseconds(200)))
    }

    @Test func `a pause far away is aiming at something, so it stays shut`() {
        #expect(!Wheel.isWondering(start: start, at: CGPoint(x: 900, y: 600), stillFor: .seconds(2)))
    }
}

struct ScreenshotTests {
    @Test func `only the three newest stay`() {
        let screenshots = Screenshots()
        let taken = (1...4).map { URL(fileURLWithPath: "/tmp/Screenshot \($0).png") }
        taken.forEach(screenshots.remember)
        #expect(screenshots.recent == taken.reversed().prefix(3).map(\.self))
    }

    @Test func `a screenshot leaves the row once another app takes it`() async throws {
        let screenshots = Screenshots()
        let (kept, used) = (URL(fileURLWithPath: "/tmp/Screenshot kept.png"), URL(fileURLWithPath: "/tmp/Screenshot used.png"))
        [kept, used].forEach(screenshots.remember)
        let drag = screenshots.dragItem(for: used)
        #expect(screenshots.recent.count == 2, "starting a drag isn't using it")

        // other apps read a dragged file's url as its bytes
        let taken = try await drag.loadItem(forTypeIdentifier: "public.file-url") as? Data
        #expect(taken.flatMap { URL(dataRepresentation: $0, relativeTo: nil) } == used)
        await Task.yield()
        #expect(screenshots.recent == [kept])
    }

    @Test func `thumbnails are small, whatever the screenshot's size`() async throws {
        let png = FileManager.default.temporaryDirectory.appending(path: "pavo-big-\(UUID().uuidString).png")
        defer { try? FileManager.default.removeItem(at: png) }
        let big = try #require(CGContext(data: nil, width: 5120, height: 2880, bitsPerComponent: 8, bytesPerRow: 0,
                                         space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)?.makeImage())
        try #require(NSBitmapImageRep(cgImage: big).representation(using: .png, properties: [:])).write(to: png)

        let thumbnail = try #require(await Screenshots.thumbnail(of: png, size: CGSize(width: 96, height: 60)))
        #expect(thumbnail.width <= 192 && thumbnail.height <= 120)
    }
}
