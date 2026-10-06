import AVFoundation
import SwiftUI

/// What opens under the menu bar icon, drawn like the website: pencil on near-black.
struct PanelView: View {
    let converter: Converter
    let chooseFiles: () -> Void
    let screenshots: Screenshots
    let close: () -> Void
    let quit: () -> Void
    let resized: (CGSize) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            content
        }
        .padding(18)
        .frame(width: 340, alignment: .leading)
        .background(Ink.paper, in: RoundedRectangle(cornerRadius: 14))
        .overlay { SketchRect(seed: "panel", roughness: 1.4).stroke(Ink.pencil.opacity(0.85), lineWidth: 1.2) }
        .padding(6)
        .fixedSize(horizontal: false, vertical: true)
        .onGeometryChange(for: CGSize.self) { $0.size } action: { resized($0) }
        .environment(\.colorScheme, .dark)
    }

    @ViewBuilder private var content: some View {
        switch converter.phase {
        case .empty:
            DropHere(converter: converter, chooseFiles: chooseFiles, screenshots: screenshots, quit: quit)
        case .loading(let urls):
            Header(urls.displayName)
            Text("reading…").font(Ink.font(13, bold: false)).foregroundStyle(Ink.faded)
        case .choosing(let urls, let actions):
            Choices(urls: urls, actions: actions, converter: converter, close: close)
        case .working(let conversion):
            Working(name: conversion.name, progress: converter.progress, cancel: converter.cancel)
        case .done(let urls):
            Done(urls: urls, again: converter.reset, close: close)
        case .failed(let message):
            Header("couldn't do that")
            Text(message).font(Ink.font(13, bold: false)).foregroundStyle(Ink.faded).fixedSize(horizontal: false, vertical: true)
            Button("try another file", action: converter.reset).buttonStyle(SketchButtonStyle(seed: "again"))
        }
    }
}

private struct Header: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text)
            .font(Ink.font(20))
            .foregroundStyle(Ink.pencil)
            .lineLimit(1)
            .truncationMode(.middle)
    }
}

private struct DropHere: View {
    let converter: Converter
    let chooseFiles: () -> Void
    let screenshots: Screenshots
    let quit: () -> Void
    @State private var targeted = false

    var body: some View {
        VStack(spacing: 6) {
            Peacock().frame(height: 96)
            Text("pavo lives in your menu bar").font(Ink.font(12, bold: false)).foregroundStyle(Ink.faded)
            Text("drop a file here").font(Ink.font(20))
            Text("or on the peacock in your menu bar").font(Ink.font(12, bold: false)).foregroundStyle(Ink.faded)
        }
        .foregroundStyle(Ink.pencil)
        .frame(maxWidth: .infinity)
        .padding(.vertical, 18)
        .sketchBox("drop", shaded: targeted, dashed: true)
        .dropDestination(for: URL.self) { urls, _ in
            Task { await converter.load(urls) }
            return true
        } isTargeted: { targeted = $0 }

        if Bundle.main.isTemporaryCopy {
            Text("you're running pavo from the disk image. drag it into applications first, then open it from there.")
                .font(Ink.font(12))
                .foregroundStyle(Ink.pencil)
                .fixedSize(horizontal: false, vertical: true)
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .sketchBox("from-dmg")
        }

        HStack {
            Button("choose files…", action: chooseFiles).buttonStyle(SketchButtonStyle(seed: "choose"))
            Spacer()
            Button("quit", action: quit)
                .buttonStyle(.plain)
                .font(Ink.font(12, bold: false))
                .foregroundStyle(Ink.faded)
        }
        let recent = screenshots.recent.filter { FileManager.default.fileExists(atPath: $0.path) }
        if !recent.isEmpty {
            RecentScreenshots(screenshots: recent, drag: screenshots.dragItem(for:)) { screenshot in
                screenshots.forget(screenshot)
                Task { await converter.load([screenshot]) }
            }
        }

        Toggle(isOn: Binding(get: { screenshots.isOn }, set: { screenshots.turn(on: $0) })) {
            Text("put new screenshots on the clipboard").font(Ink.font(12, bold: false)).foregroundStyle(Ink.pencil)
        }
        .toggleStyle(.switch)
        .controlSize(.mini)
        .help("take a screenshot, then ⌘V it anywhere. desktop screenshots are also filed into Pictures/Screenshots.")

        Text("tip: drag any file and pause for a moment, and the wheel opens right there. ⇧ opens it straight away, ⌥ shows tools.")
            .font(Ink.font(11, bold: false))
            .foregroundStyle(Ink.faded)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// The last few screenshots, ready to drag into any app, or click to convert.
private struct RecentScreenshots: View {
    let screenshots: [URL]
    let drag: (URL) -> NSItemProvider
    let pick: (URL) -> Void

    var body: some View {
        Text("recent screenshots · drag one anywhere").font(Ink.font(12, bold: false)).foregroundStyle(Ink.faded)
        HStack(spacing: 8) {
            ForEach(screenshots, id: \.self) { screenshot in
                Thumbnail(screenshot: screenshot)
                    .onTapGesture { pick(screenshot) }
                    .onDrag { drag(screenshot) }
                    .help(screenshot.lastPathComponent)
            }
        }
    }
}

private struct Thumbnail: View {
    static let size = CGSize(width: 96, height: 60)
    let screenshot: URL
    @State private var image: CGImage?

    var body: some View {
        ZStack {
            if let image {
                Image(decorative: image, scale: 2).resizable().scaledToFill()
            }
        }
        .frame(width: Self.size.width, height: Self.size.height)
        .clipped()
        .sketchBox(screenshot.lastPathComponent)
        .contentShape(Rectangle())
        .task(id: screenshot) { image = await Screenshots.thumbnail(of: screenshot, size: Self.size) }
    }
}

private struct Choices: View {
    let urls: [URL]
    let actions: [Engine.Action]
    let converter: Converter
    let close: () -> Void

    @State private var selection = 0
    @State private var trimming = false
    @State private var compressing = false
    @State private var unwatermarking = false
    @FocusState private var focused: Bool

    var body: some View {
        Header(urls.displayName)
        if actions.isEmpty {
            Text("nothing to do with this yet").font(Ink.font(13, bold: false)).foregroundStyle(Ink.faded)
        }
        section("convert to", group: "convert", columns: 4)
        section("edit", group: "edit", columns: 2)
        section("more", group: "tool", columns: 2)
        if trimming {
            TrimFields { range in converter.start("trim:\(range)", on: urls) }
        }
        if compressing {
            CompressLevels { level in converter.start(level, on: urls) }
        }
        if unwatermarking, let file = urls.first {
            WatermarkOptions(file: file) { action in converter.start(action, on: urls) }
        }
        HStack {
            Button("← another file", action: converter.reset).buttonStyle(.plain)
            Spacer()
            Text("arrows · enter · esc")
        }
        .font(Ink.font(11, bold: false))
        .foregroundStyle(Ink.faded)
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onAppear { focused = true }
        .onKeyPress(.leftArrow) { move(-1) }
        .onKeyPress(.upArrow) { move(-1) }
        .onKeyPress(.rightArrow) { move(1) }
        .onKeyPress(.downArrow) { move(1) }
        .onKeyPress(.return) {
            if actions.indices.contains(selection) { pick(actions[selection]) }
            return .handled
        }
    }

    @ViewBuilder
    private func section(_ title: String, group: String, columns: Int) -> some View {
        let items = actions.enumerated().filter { $0.element.group == group }
        if !items.isEmpty {
            Text(title).font(Ink.font(12, bold: false)).foregroundStyle(Ink.faded)
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: columns), spacing: 8) {
                ForEach(items, id: \.element.id) { index, action in
                    Button { pick(action) } label: {
                        Text(action.label).lineLimit(1).minimumScaleFactor(0.7).frame(maxWidth: .infinity)
                    }
                    .buttonStyle(SketchButtonStyle(seed: action.id, highlighted: index == selection && focused, size: 13))
                }
            }
        }
    }

    private func move(_ step: Int) -> KeyPress.Result {
        guard !actions.isEmpty else { return .ignored }
        selection = (selection + step + actions.count) % actions.count
        return .handled
    }

    private func pick(_ action: Engine.Action) {
        if action.id == "trim" {
            trimming = true // needs a start and an end first
        } else if action.id == "compress" {
            compressing = true // light, balanced or smallest
        } else if action.id == "unwatermark" {
            unwatermarking = true // automatic, by text (pdf) or by box (video)
        } else {
            converter.start(action.id, on: urls)
        }
    }
}

/// Remove watermark: let pavo find it, or point at it. Pdfs take the watermark's text;
/// videos take a box drawn on one of their frames.
private struct WatermarkOptions: View {
    let file: URL
    let remove: (String) -> Void
    @State private var text = ""

    private var isPDF: Bool { file.pathExtension.lowercased() == "pdf" }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button("find it automatically") { remove("unwatermark") }
                .buttonStyle(SketchButtonStyle(seed: "unwatermark-auto", size: 13))
            if isPDF {
                Text("or remove this text").font(Ink.font(12, bold: false)).foregroundStyle(Ink.faded)
                HStack(spacing: 8) {
                    TextField("CONFIDENTIAL", text: $text)
                        .textFieldStyle(.plain)
                        .font(Ink.font(13, bold: false))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 6)
                        .sketchBox("watermark-text")
                        .onSubmit(removeText)
                    Button("remove", action: removeText)
                        .buttonStyle(SketchButtonStyle(seed: "unwatermark-text", size: 13))
                        .disabled(text.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            } else {
                Text("or drag a box around it").font(Ink.font(12, bold: false)).foregroundStyle(Ink.faded)
                FramePicker(video: file) { box in
                    remove(String(format: "unwatermark:%.4f,%.4f,%.4f,%.4f", box.minX, box.minY, box.width, box.height))
                }
            }
        }
    }

    private func removeText() {
        let words = text.trimmingCharacters(in: .whitespaces)
        if !words.isEmpty {
            remove("unwatermark:text=\(words)")
        }
    }
}

/// One frame of the video to draw a box on. The box is handed back as fractions of the frame.
struct FramePicker: View {
    let video: URL
    let picked: (CGRect) -> Void

    @State private var frame: CGImage?
    @State private var unreadable = false
    @State private var start: CGPoint?
    @State private var box: CGRect?
    private let width: CGFloat = 304

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let frame {
                let height = width * CGFloat(frame.height) / CGFloat(frame.width)
                Image(decorative: frame, scale: 1)
                    .resizable()
                    .frame(width: width, height: height)
                    .overlay(alignment: .topLeading) {
                        if let box {
                            Rectangle()
                                .fill(Ink.pencil.opacity(0.18))
                                .overlay { SketchRect(seed: "watermark-box", roughness: 0.6).stroke(Ink.pencil, lineWidth: 1.4) }
                                .frame(width: box.width * width, height: box.height * height)
                                .offset(x: box.minX * width, y: box.minY * height)
                        }
                    }
                    .contentShape(Rectangle())
                    .gesture(
                        DragGesture(minimumDistance: 2)
                            .onChanged { drag in
                                let size = CGSize(width: width, height: height)
                                let a = start ?? drag.startLocation
                                start = a
                                box = Self.fraction(from: a, to: drag.location, in: size)
                            }
                            .onEnded { _ in start = nil }
                    )
                    .sketchBox("frame")
                Button("remove this area") { if let box { picked(box) } }
                    .buttonStyle(SketchButtonStyle(seed: "unwatermark-box", size: 13))
                    .disabled(box == nil)
            } else {
                Text(unreadable ? "couldn't read a frame from this video" : "loading a frame…")
                    .font(Ink.font(12, bold: false))
                    .foregroundStyle(Ink.faded)
            }
        }
        .task {
            frame = await Self.still(of: video)
            unreadable = frame == nil
        }
    }

    /// A box between two points, clamped to the picture, as fractions of it.
    static func fraction(from a: CGPoint, to b: CGPoint, in size: CGSize) -> CGRect {
        let clamp = { (p: CGPoint) in CGPoint(x: min(max(p.x, 0), size.width), y: min(max(p.y, 0), size.height)) }
        let (p, q) = (clamp(a), clamp(b))
        return CGRect(
            x: min(p.x, q.x) / size.width,
            y: min(p.y, q.y) / size.height,
            width: abs(q.x - p.x) / size.width,
            height: abs(q.y - p.y) / size.height
        )
    }

    /// A small frame from a second in, the right way up.
    static func still(of video: URL) async -> CGImage? {
        let generator = AVAssetImageGenerator(asset: AVURLAsset(url: video))
        generator.appliesPreferredTrackTransform = true
        generator.maximumSize = CGSize(width: 640, height: 640)
        return try? await generator.image(at: CMTime(seconds: 1, preferredTimescale: 600)).image
    }
}

private struct CompressLevels: View {
    let compress: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("how small?").font(Ink.font(12, bold: false)).foregroundStyle(Ink.faded)
            HStack(spacing: 8) {
                level("light", "compress:light", "barely touches quality")
                level("balanced", "compress", "looks the same")
                level("smallest", "compress:smallest", "as small as it gets")
            }
        }
    }

    private func level(_ title: String, _ action: String, _ hint: String) -> some View {
        Button { compress(action) } label: {
            VStack(spacing: 1) {
                Text(title)
                Text(hint).font(Ink.font(10, bold: false)).foregroundStyle(Ink.faded).lineLimit(1).minimumScaleFactor(0.7)
            }
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(SketchButtonStyle(seed: action, size: 13))
    }
}

private struct TrimFields: View {
    let trim: (String) -> Void
    @State private var from = "0:00"
    @State private var to = "0:10"

    var body: some View {
        HStack(spacing: 8) {
            field("from", $from)
            field("to", $to)
            Button("trim") { trim("\(from)-\(to)") }.buttonStyle(SketchButtonStyle(seed: "trim-go"))
        }
    }

    private func field(_ label: String, _ text: Binding<String>) -> some View {
        HStack(spacing: 4) {
            Text(label).foregroundStyle(Ink.faded)
            TextField("0:00", text: text)
                .textFieldStyle(.plain)
                .frame(width: 52)
                .padding(.horizontal, 6)
                .padding(.vertical, 4)
                .sketchBox("field-\(label)")
        }
        .font(Ink.font(13, bold: false))
        .foregroundStyle(Ink.pencil)
    }
}

private struct Working: View {
    let name: String
    let progress: Double
    let cancel: () -> Void

    var body: some View {
        Header("converting…")
        Text(name).font(Ink.font(13, bold: false)).foregroundStyle(Ink.faded).lineLimit(1).truncationMode(.middle)
        GeometryReader { proxy in
            Hachure(seed: "progress", gap: 4)
                .stroke(Ink.pencil.opacity(0.7), lineWidth: 1.1)
                .frame(width: max(0, proxy.size.width * progress))
                .clipped()
        }
        .frame(height: 22)
        .sketchBox("progress-bar")
        HStack {
            Text("\(Int(progress * 100))%").font(Ink.font(15)).monospacedDigit()
            Spacer()
            Button("cancel", action: cancel).buttonStyle(SketchButtonStyle(seed: "cancel"))
        }
        .foregroundStyle(Ink.pencil)
    }
}

private struct Done: View {
    let urls: [URL]
    let again: () -> Void
    let close: () -> Void

    var body: some View {
        Header("done ✓")
        Text(urls.count == 1 ? "saved \(urls[0].lastPathComponent) next to the original" : "saved \(urls.count) files next to the originals")
            .font(Ink.font(13, bold: false))
            .foregroundStyle(Ink.faded)
            .fixedSize(horizontal: false, vertical: true)
        HStack {
            Button("show in finder") {
                NSWorkspace.shared.activateFileViewerSelecting(urls)
                close()
            }
            .buttonStyle(SketchButtonStyle(seed: "reveal"))
            Button("another", action: again).buttonStyle(SketchButtonStyle(seed: "another"))
        }
    }
}
