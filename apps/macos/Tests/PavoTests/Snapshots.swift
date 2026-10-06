import AppKit
import SwiftUI
import Testing

@testable import Pavo

/// Renders the panel and the wheel to pngs so the sketch style can be checked by eye.
/// Run with `PAVO_SNAPSHOTS=/some/folder swift test --filter Snapshots` (after `cargo build --release`).
@Suite(.enabled(if: ProcessInfo.processInfo.environment["PAVO_SNAPSHOTS"] != nil))
struct Snapshots {
    let output = URL(fileURLWithPath: ProcessInfo.processInfo.environment["PAVO_SNAPSHOTS"] ?? "/tmp")
    let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../../..").standardized

    init() throws {
        setenv("PAVO_CLI", repo.appending(path: "target/release/pavo").path, 1)
    }

    @Test func `panel and wheel`() async throws {
        let files = FileManager.default.temporaryDirectory.appending(path: "pavo-snap-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: files, withIntermediateDirectories: true)
        let video = files.appending(path: "holiday.mov")
        let pdf = files.appending(path: "report.pdf")
        for file in [video, pdf] { try Data().write(to: file) }

        try save(panel(Converter()), "panel-empty")
        let screenshots = Screenshots()
        for name in ["one", "two", "three"] {
            let screenshot = files.appending(path: "Screenshot \(name).png")
            try Data().write(to: screenshot)
            screenshots.remember(screenshot)
        }
        try save(panel(Converter(), screenshots: screenshots), "panel-screenshots")

        let converter = Converter()
        await converter.load([video])
        try save(panel(converter), "panel-video")

        let pdfs = Converter()
        await pdfs.load([pdf])
        try save(panel(pdfs), "panel-pdf")

        let model = WheelModel()
        model.urls = [video]
        model.actions = await Converter().actions(for: [video])
        model.hovered = "to:mp4"
        try save(WheelView(model: model, size: 400) { _ in }, "wheel-formats")
        model.showingTools = true
        model.hovered = nil
        try save(WheelView(model: model, size: 400) { _ in }, "wheel-tools")
    }

    private func panel(_ converter: Converter, screenshots: Screenshots = Screenshots()) -> some View {
        PanelView(converter: converter, chooseFiles: {}, screenshots: screenshots, close: {}, quit: {}, resized: { _ in })
    }

    private func save(_ view: some View, _ name: String) throws {
        let renderer = ImageRenderer(content: view.background(Color(white: 0.3)))
        renderer.scale = 2
        let image = try #require(renderer.nsImage)
        let bitmap = try #require(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
        try #require(bitmap.representation(using: .png, properties: [:])).write(to: output.appending(path: "\(name).png"))
    }
}
