// One config key as a control, chosen from the daemon's field metadata.
// Text-like controls commit on Return or focus loss so the daemon is not
// asked to rewrite the file on every keystroke.

import SwiftUI

struct FieldRow: View {
    let field: FieldMeta
    let value: JSONValue?
    let defaultValue: JSONValue?
    let onSet: (JSONValue) -> Void
    let onReset: () -> Void

    private var isDefault: Bool {
        value == nil || value == defaultValue
    }

    var body: some View {
        LabeledContent {
            HStack(spacing: 8) {
                control
                if !isDefault {
                    Button("Reset") { onReset() }
                        .controlSize(.small)
                        .help("Back to the default: \(defaultValue?.display ?? "—")")
                }
            }
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                Text(field.name).font(.body)
                Text(field.doc).font(.caption).foregroundStyle(.secondary)
                if field.appliesLater {
                    Text("Stored and validated now; applies from \(field.milestone ?? "a later milestone").")
                        .font(.caption2)
                        .foregroundStyle(.orange)
                }
            }
        }
    }

    @ViewBuilder private var control: some View {
        switch field.kind {
        case "bool":
            Toggle("", isOn: Binding(get: { value?.boolValue ?? false }, set: { onSet(.bool($0)) }))
                .labelsHidden()
        case "enum":
            Picker("", selection: Binding(get: { value?.stringValue ?? "" }, set: { onSet(.string($0)) })) {
                ForEach(field.options ?? [], id: \.self) { Text($0).tag($0) }
            }
            .labelsHidden()
            .frame(maxWidth: 220)
        case "int", "float":
            NumberField(field: field, value: value?.doubleValue ?? 0, onSet: { onSet(.number($0)) })
        case "list":
            DraftTextField(text: (value?.stringArray ?? []).joined(separator: ", "), placeholder: "a, b, c", monospaced: false) { text in
                onSet(.array(text.split(separator: ",").map { .string($0.trimmingCharacters(in: .whitespaces)) }
                        .filter { $0.stringValue?.isEmpty == false }))
            }
        case "hotkey":
            DraftTextField(text: value?.stringValue ?? "", placeholder: "cmd+shift+a", monospaced: true) { onSet(.string($0)) }
        default:
            DraftTextField(text: value?.stringValue ?? "", placeholder: "", monospaced: false) { onSet(.string($0)) }
        }
    }
}

/// A text field whose draft only reaches the daemon on commit.
struct DraftTextField: View {
    let text: String
    let placeholder: String
    let monospaced: Bool
    let onCommit: (String) -> Void
    @State private var draft = ""
    @FocusState private var focused: Bool

    var body: some View {
        TextField("", text: $draft, prompt: placeholder.isEmpty ? nil : Text(placeholder))
            .textFieldStyle(.roundedBorder)
            .font(monospaced ? .body.monospaced() : .body)
            .frame(minWidth: 220, maxWidth: 360)
            .focused($focused)
            .onAppear { draft = text }
            .onChange(of: text) { _, new in
                if !focused {
                    draft = new
                }
            }
            .onSubmit { commit() }
            .onChange(of: focused) { _, now in
                if !now {
                    commit()
                }
            }
    }

    private func commit() {
        guard draft != text else { return }
        onCommit(draft)
    }
}

struct NumberField: View {
    let field: FieldMeta
    let value: Double
    let onSet: (Double) -> Void
    @State private var draft = ""
    @FocusState private var focused: Bool

    private var isInt: Bool {
        field.kind == "int"
    }

    private var step: Double {
        field.step ?? 1
    }

    var body: some View {
        HStack(spacing: 4) {
            TextField("", text: $draft)
                .textFieldStyle(.roundedBorder)
                .font(.body.monospaced())
                .multilineTextAlignment(.trailing)
                .frame(width: 110)
                .focused($focused)
                .onAppear { draft = format(value) }
                .onChange(of: value) { _, new in
                    if !focused {
                        draft = format(new)
                    }
                }
                .onSubmit { commit() }
                .onChange(of: focused) { _, now in
                    if !now {
                        commit()
                    }
                }
            Stepper("", onIncrement: { onSet(clamp(value + step)) }, onDecrement: { onSet(clamp(value - step)) })
                .labelsHidden()
            if let min = field.min, let max = field.max {
                Text("\(format(min))–\(format(max))").font(.caption2).foregroundStyle(.tertiary)
            }
        }
    }

    private func format(_ v: Double) -> String {
        isInt ? String(Int64(v)) : String(format: "%g", v)
    }

    private func clamp(_ v: Double) -> Double {
        var out = v
        if let min = field.min {
            out = Swift.max(min, out)
        }
        if let max = field.max {
            out = Swift.min(max, out)
        }
        return isInt ? out.rounded() : (out * 1000).rounded() / 1000
    }

    private func commit() {
        guard let parsed = Double(draft.trimmingCharacters(in: .whitespaces)) else {
            draft = format(value)
            return
        }
        let clamped = clamp(parsed)
        draft = format(clamped)
        if clamped != value {
            onSet(clamped)
        }
    }
}
