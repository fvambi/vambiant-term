// Themes: pick the dark and light theme, preview every colour, and edit
// user themes in place. Built-ins are read-only; duplicate one to start.

import AppKit
import SwiftUI

struct ThemesView: View {
    let model: ConfigModel
    @State private var selected: String?
    @State private var draft: ThemeFile?
    @State private var copyName = ""

    private static let builtins: Set<String> = ["vambiant-dark", "vambiant-light"]

    var body: some View {
        let themes = (model.snapshot?.themes ?? [:]).values.sorted { $0.name < $1.name }
        let darkName = model.value("theme.name")?.stringValue ?? ""
        let lightName = model.value("theme.light")?.stringValue ?? ""
        HSplitView {
            List(themes, selection: $selected) { theme in
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text(theme.name)
                        if theme.name == darkName {
                            Text("dark").font(.caption2).padding(2).background(.quaternary).cornerRadius(3)
                        }
                        if theme.name == lightName {
                            Text("light").font(.caption2).padding(2).background(.quaternary).cornerRadius(3)
                        }
                    }
                    Swatches(theme: theme)
                }
                .tag(theme.name)
            }
            .frame(minWidth: 220, idealWidth: 260)
            .onChange(of: selected) { _, name in
                draft = name.flatMap { model.snapshot?.themes[$0] }
            }
            .onChange(of: model.snapshot?.themes) { _, themes in
                if draft == nil, let name = selected {
                    draft = themes?[name]
                }
            }
            editor
                .frame(minWidth: 380)
        }
        .onAppear {
            if selected == nil {
                selected = darkName
                draft = model.snapshot?.themes[darkName]
            }
        }
    }

    @ViewBuilder private var editor: some View {
        if let theme = draft {
            let readOnly = Self.builtins.contains(theme.name)
            Form {
                Section("Use") {
                    HStack {
                        Button("Use for dark mode") { model.set("theme.name", .string(theme.name)) }
                        Button("Use for light mode") { model.set("theme.light", .string(theme.name)) }
                        Toggle("Follow system appearance", isOn: Binding(
                            get: { model.value("theme.follow_system")?.boolValue ?? true },
                            set: { model.set("theme.follow_system", .bool($0)) }
                        ))
                    }
                }
                Section(readOnly ? "Colours (built-in, read-only — duplicate to edit)" : "Colours") {
                    colourRow("background", \.background, readOnly)
                    colourRow("foreground", \.foreground, readOnly)
                    colourRow("cursor", \.cursor, readOnly)
                    colourRow("selection", \.selection, readOnly)
                    ansiRows("normal", \.normal, readOnly)
                    ansiRows("bright", \.bright, readOnly)
                    colourRow("ui.accent", \.ui.accent, readOnly)
                    colourRow("ui.warning", \.ui.warning, readOnly)
                    colourRow("ui.danger", \.ui.danger, readOnly)
                    colourRow("ui.success", \.ui.success, readOnly)
                }
                Section {
                    HStack {
                        if !readOnly {
                            Button("Save \(theme.name).toml") { model.save(theme: theme) }
                                .disabled(model.snapshot?.themes[theme.name] == theme)
                        }
                        Spacer()
                        TextField("", text: $copyName, prompt: Text("new name")).frame(width: 160)
                        Button("Duplicate") {
                            var copy = theme
                            copy.name = copyName.trimmingCharacters(in: .whitespaces)
                            model.save(theme: copy)
                            selected = copy.name
                            draft = copy
                            copyName = ""
                        }
                        .disabled(copyName.trimmingCharacters(in: .whitespaces).isEmpty || Self.builtins.contains(copyName))
                    }
                    Text(model.snapshot?.paths.themes ?? "").font(.caption.monospaced()).foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
        } else {
            Text("Select a theme").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func colourRow(_ label: String, _ path: WritableKeyPath<ThemeFile, String>, _ readOnly: Bool) -> some View {
        LabeledContent(label) {
            HStack {
                ColorPicker("", selection: Binding(
                    get: { Color(hex: draft?[keyPath: path] ?? "#000000") },
                    set: {
                        if !readOnly {
                            draft?[keyPath: path] = $0.hexString
                        }
                    }
                ), supportsOpacity: false)
                    .labelsHidden()
                    .disabled(readOnly)
                Text(draft?[keyPath: path] ?? "").font(.caption.monospaced()).foregroundStyle(.secondary)
            }
        }
    }

    @ViewBuilder private func ansiRows(_ group: String, _ path: WritableKeyPath<ThemeFile, AnsiFile>, _ readOnly: Bool) -> some View {
        colourRow("\(group).black", path.appending(path: \.black), readOnly)
        colourRow("\(group).red", path.appending(path: \.red), readOnly)
        colourRow("\(group).green", path.appending(path: \.green), readOnly)
        colourRow("\(group).yellow", path.appending(path: \.yellow), readOnly)
        colourRow("\(group).blue", path.appending(path: \.blue), readOnly)
        colourRow("\(group).magenta", path.appending(path: \.magenta), readOnly)
        colourRow("\(group).cyan", path.appending(path: \.cyan), readOnly)
        colourRow("\(group).white", path.appending(path: \.white), readOnly)
    }
}

struct Swatches: View {
    let theme: ThemeFile

    var body: some View {
        HStack(spacing: 2) {
            ForEach(Array((theme.normal.ordered + theme.bright.ordered).enumerated()), id: \.offset) { _, hex in
                RoundedRectangle(cornerRadius: 2).fill(Color(hex: hex)).frame(width: 12, height: 12)
            }
        }
        .padding(3)
        .background(Color(hex: theme.background))
        .cornerRadius(3)
    }
}

extension Color {
    init(hex: String) {
        let rgb = RGBA(hexString: hex) ?? RGBA(r: 0, g: 0, b: 0)
        self.init(.sRGB, red: Double(rgb.r), green: Double(rgb.g), blue: Double(rgb.b), opacity: 1)
    }

    var hexString: String {
        let ns = NSColor(self).usingColorSpace(.sRGB) ?? .black
        return String(
            format: "#%02x%02x%02x",
            Int((ns.redComponent * 255).rounded()),
            Int((ns.greenComponent * 255).rounded()),
            Int((ns.blueComponent * 255).rounded())
        )
    }
}
