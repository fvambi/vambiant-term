// The Settings window: every config.toml section as a form built from the
// daemon's field metadata, plus the keymap and theme editors.

import SwiftUI

enum SettingsSection: Hashable {
    case config(String)
    case keymap
    case themes

    static let titles: [String: String] = [
        "font": "Font", "theme": "Theme", "window": "Window", "cursor": "Cursor",
        "terminal": "Terminal", "shell_integration": "Shell integration", "mux": "Multiplexer",
        "agents": "Agents", "notifications": "Notifications", "ai": "AI", "privacy": "Privacy",
        "storage": "Storage", "api": "Local API",
    ]

    var title: String {
        switch self {
        case let .config(id): return Self.titles[id] ?? id
        case .keymap: return "Keymap"
        case .themes: return "Themes"
        }
    }
}

struct SettingsView: View {
    @Bindable var model: ConfigModel
    @State private var selection: SettingsSection? = .config("font")

    var body: some View {
        NavigationSplitView {
            List(selection: $selection) {
                Section("config.toml") {
                    ForEach(model.snapshot?.sections ?? [], id: \.self) { id in
                        Text(SettingsSection.config(id).title).tag(SettingsSection.config(id))
                    }
                }
                Section("Files") {
                    Text("Keymap").tag(SettingsSection.keymap)
                    Text("Themes").tag(SettingsSection.themes)
                }
            }
            .listStyle(.sidebar)
            .navigationSplitViewColumnWidth(min: 170, ideal: 190)
        } detail: {
            VStack(spacing: 0) {
                StatusBanner(model: model)
                switch selection {
                case let .config(id): ConfigSectionView(model: model, section: id)
                case .keymap: KeymapView(model: model)
                case .themes: ThemesView(model: model)
                case nil: Text("Choose a section").foregroundStyle(.secondary)
                }
            }
            .navigationTitle(selection?.title ?? "Settings")
        }
        .frame(minWidth: 820, minHeight: 520)
        .onAppear { model.reload() }
    }
}

/// Parse errors, validation warnings and the last refused edit — always
/// visible, never a silent fallback.
struct StatusBanner: View {
    let model: ConfigModel

    /// Built imperatively: one long array expression made Swift 6.1 give
    /// up type-checking it.
    static func problems(in model: ConfigModel) -> [(String, Color)] {
        var out: [(String, Color)] = []
        let snap = model.snapshot
        if let e = snap?.configError {
            out.append(("config.toml is not being applied — \(e)", .red))
        }
        if let e = snap?.keymapError {
            out.append(("keymap.toml is not being applied — \(e)", .red))
        }
        for w in snap?.configWarnings ?? [] {
            out.append(("config.toml: \(w)", .orange))
        }
        for w in snap?.themeWarnings ?? [] {
            out.append(("themes: \(w)", .orange))
        }
        if let e = model.lastError {
            out.append(("refused: \(e)", .red))
        }
        return out
    }

    var body: some View {
        let problems = Self.problems(in: model)
        if !problems.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(problems.enumerated()), id: \.offset) { _, p in
                    Label(p.0, systemImage: p.1 == .red ? "xmark.octagon.fill" : "exclamationmark.triangle.fill")
                        .foregroundStyle(p.1)
                        .font(.callout)
                        .textSelection(.enabled)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(10)
            .background(.quaternary)
        }
    }
}

struct ConfigSectionView: View {
    let model: ConfigModel
    let section: String

    var body: some View {
        let fields = (model.snapshot?.fields ?? []).filter { $0.section == section }
        Form {
            ForEach(fields) { field in
                FieldRow(
                    field: field,
                    value: model.value(field.path),
                    defaultValue: model.defaultValue(field.path),
                    onSet: { model.set(field.path, $0) },
                    onReset: { model.reset(field.path) }
                )
            }
            Section {
                HStack {
                    Text(model.snapshot?.paths.config ?? "")
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                    Spacer()
                    Button("Reveal in Finder") { model.revealConfig() }
                    Button("Reload") { model.reload() }
                }
            }
        }
        .formStyle(.grouped)
    }
}
