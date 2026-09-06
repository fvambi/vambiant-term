// Workflows in the app (12 §B12): the daemon's `workflows.list` reply, a
// `{{arg}}` renderer that mirrors vt-workflows, and the sheet that asks
// for arguments. A workflow is staged into the editor, never run here.

import AppKit

struct WorkflowArgument: Decodable, Equatable, Sendable {
    let name: String
    var description = ""
    var defaultValue: String?

    enum CodingKeys: String, CodingKey {
        case name, description
        case defaultValue = "default_value"
    }

    init(name: String, description: String = "", defaultValue: String? = nil) {
        self.name = name
        self.description = description
        self.defaultValue = defaultValue
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        description = try c.decodeIfPresent(String.self, forKey: .description) ?? ""
        defaultValue = try c.decodeIfPresent(String.self, forKey: .defaultValue)
    }
}

struct Workflow: Decodable, Equatable, Sendable {
    let name: String
    let command: String
    var description = ""
    var tags: [String] = []
    var arguments: [WorkflowArgument] = []
    var warp = false

    enum CodingKeys: String, CodingKey {
        case name, command, description, tags, arguments, warp
    }

    init(
        name: String,
        command: String,
        description: String = "",
        tags: [String] = [],
        arguments: [WorkflowArgument] = [],
        warp: Bool = false
    ) {
        self.name = name
        self.command = command
        self.description = description
        self.tags = tags
        self.arguments = arguments
        self.warp = warp
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        command = try c.decode(String.self, forKey: .command)
        description = try c.decodeIfPresent(String.self, forKey: .description) ?? ""
        tags = try c.decodeIfPresent([String].self, forKey: .tags) ?? []
        arguments = try c.decodeIfPresent([WorkflowArgument].self, forKey: .arguments) ?? []
        warp = try c.decodeIfPresent(Bool.self, forKey: .warp) ?? false
    }

    /// `{{name}}` → the given value, else the default; an unset
    /// placeholder stays visible, exactly like the daemon's renderer.
    func render(_ values: [String: String]) -> String {
        var out = command
        for arg in arguments {
            if let v = values[arg.name] ?? arg.defaultValue {
                out = out.replacingOccurrences(of: "{{\(arg.name)}}", with: v)
            }
        }
        while out.last == "\n" || out.last == " " {
            out.removeLast()
        }
        return out
    }
}

struct WorkflowsReply: Decodable {
    let workflows: [Workflow]
    let problems: [String]
}

/// Asks for a workflow's arguments, defaults prefilled, and stages the
/// rendered command through `onStage`.
@MainActor
final class WorkflowSheet: NSObject {
    let window: NSWindow
    private var fields: [(String, NSTextField)] = []
    private var workflow: Workflow?
    private let preview = NSTextField(wrappingLabelWithString: "")
    private let stage = NSButton(title: "Stage in editor", target: nil, action: nil)
    private let cancel = NSButton(title: "Cancel", target: nil, action: nil)
    private let form = NSStackView()
    var onStage: ((String) -> Void)?

    override init() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 560, height: 200), styleMask: [.titled], backing: .buffered, defer: false)
        super.init()
        let content = NSView()
        window.contentView = content
        form.orientation = .vertical
        form.alignment = .leading
        form.spacing = 8
        preview.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        preview.maximumNumberOfLines = 4
        stage.bezelStyle = .rounded
        stage.keyEquivalent = "\r"
        stage.target = self
        stage.action = #selector(stageAction(_:))
        cancel.bezelStyle = .rounded
        cancel.keyEquivalent = "\u{1b}"
        cancel.target = self
        cancel.action = #selector(cancelAction(_:))
        for v in [form, preview, stage, cancel] {
            v.translatesAutoresizingMaskIntoConstraints = false
            content.addSubview(v)
        }
        NSLayoutConstraint.activate([
            form.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 16),
            form.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -16),
            form.topAnchor.constraint(equalTo: content.topAnchor, constant: 16),
            preview.leadingAnchor.constraint(equalTo: form.leadingAnchor),
            preview.trailingAnchor.constraint(equalTo: form.trailingAnchor),
            preview.topAnchor.constraint(equalTo: form.bottomAnchor, constant: 12),
            stage.trailingAnchor.constraint(equalTo: form.trailingAnchor),
            stage.topAnchor.constraint(equalTo: preview.bottomAnchor, constant: 14),
            stage.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -16),
            cancel.trailingAnchor.constraint(equalTo: stage.leadingAnchor, constant: -8),
            cancel.centerYAnchor.constraint(equalTo: stage.centerYAnchor),
        ])
    }

    func show(_ workflow: Workflow, over parent: NSWindow) {
        guard window.sheetParent == nil else { return }
        self.workflow = workflow
        window.title = workflow.name
        for v in form.arrangedSubviews {
            form.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        fields = []
        for arg in workflow.arguments {
            let label = NSTextField(labelWithString: arg.description.isEmpty ? arg.name : "\(arg.name) — \(arg.description)")
            label.font = NSFont.systemFont(ofSize: 11)
            let field = NSTextField(string: arg.defaultValue ?? "")
            field.placeholderString = "{{\(arg.name)}}"
            field.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
            field.target = self
            field.action = #selector(fieldChanged(_:))
            field.widthAnchor.constraint(equalToConstant: 520).isActive = true
            form.addArrangedSubview(label)
            form.addArrangedSubview(field)
            fields.append((arg.name, field))
        }
        refreshPreview()
        parent.beginSheet(window) { _ in }
        if let first = fields.first?.1 {
            window.makeFirstResponder(first)
        }
    }

    private var values: [String: String] {
        Dictionary(uniqueKeysWithValues: fields.map { ($0.0, $0.1.stringValue) })
    }

    @objc private func fieldChanged(_ sender: Any?) {
        refreshPreview()
    }

    private func refreshPreview() {
        preview.stringValue = workflow?.render(values) ?? ""
    }

    @objc private func stageAction(_ sender: Any?) {
        guard let workflow else { return }
        for (_, f) in fields {
            f.window?.makeFirstResponder(nil)
        }
        onStage?(workflow.render(values))
        window.sheetParent?.endSheet(window)
    }

    @objc private func cancelAction(_ sender: Any?) {
        window.sheetParent?.endSheet(window)
    }
}
