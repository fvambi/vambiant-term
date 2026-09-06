// The command palette panel (⌘⇧P): a floating field over ranked rows.
// ↑/↓ move, ↩ picks, ⎋ closes. Items and their effects come from the app.

import AppKit

@MainActor
final class CommandPalette: NSPanel, NSSearchFieldDelegate, NSTableViewDataSource, NSTableViewDelegate {
    private let field = NSSearchField()
    private let table = NSTableView()
    private var items: [PaletteItem] = []
    private var shown: [PaletteItem] = []
    private var theme = Theme.warpDark
    var onPick: ((PaletteItem) -> Void)?

    init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 640, height: 420),
            styleMask: [.titled, .fullSizeContentView, .nonactivatingPanel], backing: .buffered, defer: false
        )
        titleVisibility = .hidden
        titlebarAppearsTransparent = true
        isMovableByWindowBackground = true
        isFloatingPanel = true
        becomesKeyOnlyIfNeeded = false
        isOpaque = true
        hasShadow = true
        let content = NSView()
        content.wantsLayer = true
        contentView = content
        field.placeholderString = "Search actions, sessions:, history:, files:"
        field.delegate = self
        field.target = self
        field.action = #selector(changed)
        field.sendsSearchStringImmediately = true
        field.font = NSFont.systemFont(ofSize: 16)
        field.translatesAutoresizingMaskIntoConstraints = false
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("item"))
        table.addTableColumn(column)
        table.headerView = nil
        table.dataSource = self
        table.delegate = self
        table.rowHeight = 30
        table.backgroundColor = .clear
        table.target = self
        table.doubleAction = #selector(pick)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(field)
        content.addSubview(scroll)
        NSLayoutConstraint.activate([
            field.topAnchor.constraint(equalTo: content.topAnchor, constant: 12),
            field.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            field.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: field.bottomAnchor, constant: 8),
            scroll.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: content.bottomAnchor),
        ])
    }

    func present(items: [PaletteItem], theme: Theme, over window: NSWindow?) {
        self.items = items
        self.theme = theme
        appearance = window?.appearance
        let surface = theme.background.mixed(with: theme.foreground, 0.06).nsColor
        backgroundColor = surface
        contentView?.layer?.backgroundColor = surface.cgColor
        field.stringValue = ""
        changed()
        if let w = window {
            let f = w.frame
            setFrameOrigin(CGPoint(x: f.midX - frame.width / 2, y: f.maxY - frame.height - 80))
        } else {
            center()
        }
        makeKeyAndOrderFront(nil)
        makeFirstResponder(field)
    }

    /// Replaces the items (async sources arriving) and re-ranks.
    func update(items: [PaletteItem]) {
        self.items = items
        changed()
    }

    @objc private func changed() {
        let (scope, text) = PaletteScope.parse(field.stringValue)
        shown = Array(PaletteRanker.rank(items, scope: scope, text: text).prefix(200))
        table.reloadData()
        if !shown.isEmpty {
            table.selectRowIndexes([0], byExtendingSelection: false)
        }
    }

    @objc private func pick() {
        let i = table.selectedRow
        guard shown.indices.contains(i) else { return }
        let item = shown[i]
        orderOut(nil)
        onPick?(item)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.moveDown(_:)):
            table.selectRowIndexes([min(table.selectedRow + 1, shown.count - 1)], byExtendingSelection: false)
            table.scrollRowToVisible(table.selectedRow)
            return true
        case #selector(NSResponder.moveUp(_:)):
            table.selectRowIndexes([max(table.selectedRow - 1, 0)], byExtendingSelection: false)
            table.scrollRowToVisible(table.selectedRow)
            return true
        case #selector(NSResponder.insertNewline(_:)):
            pick()
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            orderOut(nil)
            return true
        default:
            return false
        }
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        shown.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let item = shown[row]
        let width = tableView.bounds.width
        let cell = NSView(frame: CGRect(x: 0, y: 0, width: width, height: 30))
        let icon = NSTextField(labelWithString: {
            switch item.kind {
            case .action: "⌘"
            case .session: ">_"
            case .history: "↺"
            case .file: "📄"
            }
        }())
        icon.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .semibold)
        icon.textColor = theme.foreground.nsColor.withAlphaComponent(0.6)
        icon.frame = CGRect(x: 12, y: 6, width: 24, height: 18)
        let title = NSTextField(labelWithString: item.title)
        title.font = NSFont.systemFont(ofSize: 13)
        title.textColor = theme.foreground.nsColor
        title.lineBreakMode = .byTruncatingMiddle
        title.frame = CGRect(x: 40, y: 6, width: width - 40 - 170, height: 18)
        title.autoresizingMask = [.width]
        let detail = NSTextField(labelWithString: item.detail)
        detail.font = NSFont.systemFont(ofSize: 11)
        detail.textColor = theme.foreground.nsColor.withAlphaComponent(0.55)
        detail.alignment = .right
        detail.lineBreakMode = .byTruncatingHead
        detail.frame = CGRect(x: width - 170, y: 7, width: 158, height: 16)
        detail.autoresizingMask = [.minXMargin]
        cell.addSubview(icon)
        cell.addSubview(title)
        cell.addSubview(detail)
        return cell
    }
}
