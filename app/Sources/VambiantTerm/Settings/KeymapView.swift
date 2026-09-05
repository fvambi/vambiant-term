// The resolved keymap: defaults for the active profile(s) with keymap.toml
// overrides on top, conflicts flagged, and a way to add or remove one.

import SwiftUI

struct KeymapView: View {
    let model: ConfigModel
    @State private var newChord = ""
    @State private var newAction = ""

    var body: some View {
        let km = model.snapshot?.keymap
        let actions = model.snapshot?.actions ?? []
        Form {
            Section("Profile") {
                Picker("Active profiles", selection: Binding(
                    get: { model.value("mux.keymap_profile")?.stringValue ?? "tmux" },
                    set: { model.set("mux.keymap_profile", .string($0)) }
                )) {
                    Text("tmux (prefix)").tag("tmux")
                    Text("macOS (⌘)").tag("macos")
                    Text("both").tag("both")
                }
                LabeledContent("Prefix") {
                    DraftTextField(text: model.value("mux.prefix")?.stringValue ?? "", placeholder: "ctrl+b", monospaced: true) {
                        model.set("mux.prefix", .string($0))
                    }
                }
            }
            Section("Bindings") {
                ForEach(km?.bindings ?? []) { b in
                    HStack {
                        Text(b.chord).font(.body.monospaced()).frame(width: 190, alignment: .leading)
                        Text(actions.first { $0.id == b.action }.map { "\($0.label) (\($0.id))" } ?? b.action)
                        Spacer()
                        Text(b.source).font(.caption).foregroundStyle(.secondary)
                        if b.source == "keymap.toml" {
                            Button("Remove") { model.unbind(chord: b.chord) }.controlSize(.small)
                        }
                    }
                }
            }
            Section("Add an override") {
                HStack {
                    TextField("", text: $newChord, prompt: Text("chord, e.g. cmd+shift+a or prefix a"))
                        .textFieldStyle(.roundedBorder)
                        .font(.body.monospaced())
                    Picker("", selection: $newAction) {
                        Text("action…").tag("")
                        ForEach(actions) { a in
                            Text("\(a.label) — \(a.milestone)").tag(a.id)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 280)
                    Button("Bind") {
                        model.bind(chord: newChord.trimmingCharacters(in: .whitespaces), action: newAction)
                        newChord = ""
                    }
                    .disabled(newChord.trimmingCharacters(in: .whitespaces).isEmpty || newAction.isEmpty)
                }
                Text("Actions marked with a later milestone are bound now and beep until that milestone ships.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if let km, !km.conflicts.isEmpty || !km.errors.isEmpty {
                Section("Problems") {
                    ForEach(km.errors, id: \.self) { Label($0, systemImage: "xmark.octagon").foregroundStyle(.red) }
                    ForEach(km.conflicts, id: \.self) { Label($0, systemImage: "exclamationmark.triangle").foregroundStyle(.orange) }
                }
            }
            Section {
                Text(model.snapshot?.paths.keymap ?? "").font(.caption.monospaced()).foregroundStyle(.secondary).textSelection(.enabled)
            }
        }
        .formStyle(.grouped)
    }
}
