// The mailbox (12 §G3, docs/06 §3): every notification, All / Unread /
// Errors, ↑↓ move, ↩ opens the session, ⎋ closes, and a mark-all-read.

import AppKit

@MainActor
final class MailboxSheet: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    let window: NSWindow
    var onOpen: ((Note) -> Void)?
    var onMarkAllRead: (() -> Void)?
    private var mailbox = Mailbox()
    private var filter = Mailbox.Filter.all
    private var shown: [Note] = []
    private let segments = NSSegmentedControl(labels: ["All", "Unread", "Errors"], trackingMode: .selectOne, target: nil, action: nil)
    private let table = NSTableView()
    private let empty = NSTextField(labelWithString: "Nothing yet.")
    private let markRead = NSButton(title: "Mark all read", target: nil, action: nil)
    private let close = NSButton(title: "Close (⎋)", target: nil, action: nil)

    override init() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 620, height: 420), styleMask: [.titled], backing: .buffered, defer: false)
        super.init()
        window.title = "Notifications"
        let content = KeyView()
        content.onKey = { [weak self] key in self?.handle(key: key) ?? false }
        window.contentView = content
        segments.selectedSegment = 0
        segments.target = self
        segments.action = #selector(filterChanged(_:))
        let column = NSTableColumn(identifier: .init("note"))
        column.width = 580
        table.addTableColumn(column)
        table.headerView = nil
        table.allowsTypeSelect = false
        table.rowHeight = 44
        table.dataSource = self
        table.delegate = self
        table.doubleAction = #selector(openSelected(_:))
        table.target = self
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        markRead.bezelStyle = .rounded
        markRead.target = self
        markRead.action = #selector(markAllReadAction(_:))
        close.bezelStyle = .rounded
        close.keyEquivalent = "\u{1b}"
        close.target = self
        close.action = #selector(closeAction(_:))
        empty.alignment = .center
        for v in [segments, scroll, markRead, close, empty] {
            v.translatesAutoresizingMaskIntoConstraints = false
            content.addSubview(v)
        }
        NSLayoutConstraint.activate([
            segments.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            segments.topAnchor.constraint(equalTo: content.topAnchor, constant: 12),
            scroll.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            scroll.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: segments.bottomAnchor, constant: 10),
            scroll.bottomAnchor.constraint(equalTo: markRead.topAnchor, constant: -10),
            markRead.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            markRead.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -12),
            close.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            close.centerYAnchor.constraint(equalTo: markRead.centerYAnchor),
            empty.centerXAnchor.constraint(equalTo: content.centerXAnchor),
            empty.centerYAnchor.constraint(equalTo: content.centerYAnchor),
        ])
    }

    func update(_ mailbox: Mailbox) {
        self.mailbox = mailbox
        shown = mailbox.filtered(filter)
        table.reloadData()
        empty.isHidden = !shown.isEmpty
        window.title = mailbox.unread > 0 ? "Notifications (\(mailbox.unread) unread)" : "Notifications"
        if !shown.isEmpty, table.selectedRow < 0 {
            table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        }
    }

    @objc private func filterChanged(_ sender: Any?) {
        filter = [.all, .unread, .errors][max(0, min(2, segments.selectedSegment))]
        update(mailbox)
    }

    func handle(key: String) -> Bool {
        switch key {
        case "\r": openSelected(nil)
        case "j": move(1)
        case "k": move(-1)
        default: return false
        }
        return true
    }

    private func move(_ delta: Int) {
        guard !shown.isEmpty else { return }
        let row = max(0, min(shown.count - 1, table.selectedRow + delta))
        table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
    }

    @objc private func openSelected(_ sender: Any?) {
        let row = table.selectedRow
        guard row >= 0, row < shown.count else { return }
        onOpen?(shown[row])
        closeAction(nil)
    }

    @objc private func markAllReadAction(_ sender: Any?) {
        onMarkAllRead?()
    }

    @objc private func closeAction(_ sender: Any?) {
        window.sheetParent?.endSheet(window)
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        shown.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let note = shown[row]
        let when = Self.relative(note.at)
        let cell = NSTextField(wrappingLabelWithString: "\(note.read ? "" : "● ")\(note.title) · \(when)\n\(note.body)")
        cell.font = NSFont.systemFont(ofSize: 11)
        cell.maximumNumberOfLines = 2
        return cell
    }

    nonisolated static func relative(_ date: Date, now: Date = Date()) -> String {
        let s = Int(now.timeIntervalSince(date))
        return s < 60 ? "\(s)s ago" : s < 3600 ? "\(s / 60)m ago" : "\(s / 3600)h ago"
    }
}
