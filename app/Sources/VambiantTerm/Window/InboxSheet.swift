// The approval inbox sheet (docs/06 §3, ⌘⇧A): every pending request,
// oldest first, with its verdict, why it was asked, and the keys the spec
// names: j/k move, a allow, d deny, e edit & allow, ⎋ closes. Always-allow
// and snooze wait for M8's rules; they are not drawn as if they worked.

import AppKit

@MainActor
final class InboxSheet: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    let window: NSWindow
    var onDecide: ((InboxItem, InboxDecision) -> Void)?
    /// Whether a session's agent accepts edited input (Codex does not).
    var canEdit: ((InboxItem) -> Bool)?
    private var items: [InboxItem] = []
    private let table = NSTableView()
    private let detail = NSTextView()
    private let editor = NSTextField()
    private let allow = NSButton(title: "Allow (a)", target: nil, action: nil)
    private let deny = NSButton(title: "Deny (d)", target: nil, action: nil)
    private let edit = NSButton(title: "Edit & allow (e)", target: nil, action: nil)
    private let close = NSButton(title: "Close (⎋)", target: nil, action: nil)
    private let empty = NSTextField(labelWithString: "Nothing is waiting for you.")

    override init() {
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 760, height: 440),
            styleMask: [.titled], backing: .buffered, defer: false
        )
        super.init()
        window.title = "Approvals"
        let content = KeyView()
        content.onKey = { [weak self] key in self?.handle(key: key) ?? false }
        window.contentView = content
        let column = NSTableColumn(identifier: .init("item"))
        column.width = 300
        table.addTableColumn(column)
        table.headerView = nil
        table.allowsTypeSelect = false // j/k/a/d/e reach the sheet, not type-select
        table.dataSource = self
        table.delegate = self
        table.rowHeight = 44
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        detail.isEditable = false
        detail.font = NSFont.systemFont(ofSize: 12)
        detail.textContainerInset = CGSize(width: 8, height: 8)
        let detailScroll = NSScrollView()
        detailScroll.documentView = detail
        detailScroll.hasVerticalScroller = true
        detail.autoresizingMask = [.width]
        detail.isVerticallyResizable = true
        detail.textContainer?.widthTracksTextView = true
        editor.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        editor.placeholderString = "edited command (e)"
        for (button, action) in [
            (allow, #selector(allowAction(_:))), (deny, #selector(denyAction(_:))),
            (edit, #selector(editAction(_:))), (close, #selector(closeAction(_:))),
        ] {
            button.bezelStyle = .rounded
            button.target = self
            button.action = action
        }
        close.keyEquivalent = "\u{1b}"
        empty.font = NSFont.systemFont(ofSize: 13)
        empty.alignment = .center
        for v in [scroll, detailScroll, editor, allow, deny, edit, close, empty] {
            v.translatesAutoresizingMaskIntoConstraints = false
            content.addSubview(v)
        }
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            scroll.topAnchor.constraint(equalTo: content.topAnchor, constant: 12),
            scroll.bottomAnchor.constraint(equalTo: editor.topAnchor, constant: -10),
            scroll.widthAnchor.constraint(equalToConstant: 300),
            detailScroll.leadingAnchor.constraint(equalTo: scroll.trailingAnchor, constant: 10),
            detailScroll.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            detailScroll.topAnchor.constraint(equalTo: scroll.topAnchor),
            detailScroll.bottomAnchor.constraint(equalTo: scroll.bottomAnchor),
            editor.leadingAnchor.constraint(equalTo: scroll.leadingAnchor),
            editor.trailingAnchor.constraint(equalTo: detailScroll.trailingAnchor),
            editor.bottomAnchor.constraint(equalTo: allow.topAnchor, constant: -10),
            allow.leadingAnchor.constraint(equalTo: scroll.leadingAnchor),
            allow.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -12),
            deny.leadingAnchor.constraint(equalTo: allow.trailingAnchor, constant: 8),
            deny.centerYAnchor.constraint(equalTo: allow.centerYAnchor),
            edit.leadingAnchor.constraint(equalTo: deny.trailingAnchor, constant: 8),
            edit.centerYAnchor.constraint(equalTo: allow.centerYAnchor),
            close.trailingAnchor.constraint(equalTo: detailScroll.trailingAnchor),
            close.centerYAnchor.constraint(equalTo: allow.centerYAnchor),
            empty.centerXAnchor.constraint(equalTo: content.centerXAnchor),
            empty.centerYAnchor.constraint(equalTo: content.centerYAnchor),
        ])
    }

    func update(items new: [InboxItem]) {
        let selectedID = selected?.id
        items = new
        table.reloadData()
        window.title = items.isEmpty ? "Approvals" : "Approvals (\(items.count))"
        empty.isHidden = !items.isEmpty
        let row = items.firstIndex { $0.id == selectedID } ?? (items.isEmpty ? -1 : 0)
        if row >= 0 {
            table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        }
        refreshDetail()
    }

    var selected: InboxItem? {
        let row = table.selectedRow
        return row >= 0 && row < items.count ? items[row] : nil
    }

    private func refreshDetail() {
        guard let item = selected else {
            detail.string = ""
            editor.stringValue = ""
            for b in [allow, deny, edit] {
                b.isEnabled = false
            }
            return
        }
        var lines = [
            "\(item.sessionName) · \(item.request.tool) · \(item.hookEvent) · waiting \(item.waitingLabel)",
            "",
            item.summary,
            "",
        ]
        if let v = item.verdictLine {
            lines.append(v)
        }
        if let f = item.floorLine {
            lines.append(f)
        }
        lines.append("Asked because \(item.askedBecause).")
        if let reason = item.request.reason, !reason.isEmpty {
            lines.append("Agent says: \(reason)")
        }
        if item.promptShown {
            lines.append("The hold expired: the agent shows its own prompt. Answer it there.")
        }
        detail.string = lines.joined(separator: "\n")
        editor.stringValue = item.command ?? ""
        let editable = item.command != nil && (canEdit?(item) ?? true) && !item.promptShown
        editor.isEnabled = editable
        edit.isEnabled = editable
        allow.isEnabled = !item.promptShown
        deny.isEnabled = !item.promptShown
    }

    /// The spec's keys; returns false for anything else.
    func handle(key: String) -> Bool {
        switch key {
        case "j": move(1)
        case "k": move(-1)
        case "a": allowAction(nil)
        case "d": denyAction(nil)
        case "e": editAction(nil)
        default: return false
        }
        return true
    }

    private func move(_ delta: Int) {
        guard !items.isEmpty else { return }
        let row = max(0, min(items.count - 1, table.selectedRow + delta))
        table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
    }

    @objc private func allowAction(_ sender: Any?) {
        if let item = selected, allow.isEnabled {
            onDecide?(item, .allow(updatedCommand: nil))
        }
    }

    @objc private func denyAction(_ sender: Any?) {
        if let item = selected, deny.isEnabled {
            onDecide?(item, .deny(reason: "denied from the Vambiant Term inbox"))
        }
    }

    @objc private func editAction(_ sender: Any?) {
        guard let item = selected, edit.isEnabled else { return }
        let edited = editor.stringValue
        if edited.isEmpty || edited == item.command {
            window.makeFirstResponder(editor)
            return
        }
        onDecide?(item, .allow(updatedCommand: edited))
    }

    @objc private func closeAction(_ sender: Any?) {
        window.sheetParent?.endSheet(window)
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        items.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let item = items[row]
        let cell =
            NSTextField(wrappingLabelWithString: "\(item.sessionName) · \(item.request.tool) · \(item.waitingLabel)\n\(item.summary)")
        cell.font = NSFont.systemFont(ofSize: 11)
        cell.maximumNumberOfLines = 2
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        refreshDetail()
    }
}

/// Routes plain key presses to the sheet while the table has focus.
@MainActor
final class KeyView: NSView {
    var onKey: ((String) -> Bool)?

    override var acceptsFirstResponder: Bool {
        true
    }

    override func keyDown(with event: NSEvent) {
        if event.modifierFlags.isDisjoint(with: .deviceIndependentFlagsMask),
           let chars = event.charactersIgnoringModifiers, onKey?(chars) == true {
            return
        }
        super.keyDown(with: event)
    }
}
