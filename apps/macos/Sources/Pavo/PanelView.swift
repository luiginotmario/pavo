import SwiftUI

/// What opens under the menu bar icon, drawn like the website: pencil on near-black.
struct PanelView: View {
    let converter: Converter
    let chooseFiles: () -> Void
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
            DropHere(converter: converter, chooseFiles: chooseFiles, quit: quit)
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
    let quit: () -> Void
    @State private var targeted = false

    var body: some View {
        VStack(spacing: 6) {
            Peacock().frame(height: 96)
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

        HStack {
            Button("choose files…", action: chooseFiles).buttonStyle(SketchButtonStyle(seed: "choose"))
            Spacer()
            Button("quit", action: quit)
                .buttonStyle(.plain)
                .font(Ink.font(12, bold: false))
                .foregroundStyle(Ink.faded)
        }
        Text("tip: hold ⇧ while dragging a file anywhere for the wheel. add ⌥ for tools.")
            .font(Ink.font(11, bold: false))
            .foregroundStyle(Ink.faded)
            .fixedSize(horizontal: false, vertical: true)
    }
}

private struct Choices: View {
    let urls: [URL]
    let actions: [Engine.Action]
    let converter: Converter
    let close: () -> Void

    @State private var selection = 0
    @State private var trimming = false
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
        } else {
            converter.start(action.id, on: urls)
        }
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
