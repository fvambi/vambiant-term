// Warp's vertical tabs (12 §L, docs/06 §2): a search field over rows of
// sessions grouped by repository, each with an icon, state badge, title,
// cwd, branch and diff pill. Selection focuses the pane.

import AppKit

@MainActor
final class SidebarView: NSView, NSTableViewDataSource, NSTableViewDelegate, NSSearchFieldDelegate {
    private let search = NSSearchField()
    private let table = NSTableView()
    private let scroll = NSScrollView()
    private var allRows: [SidebarRow] = []
    private var rows: [SidebarRow] = []
    private var theme = Theme.warpDark
    var onSelect: ((ObjectIdentifier) -> Void)?
    static let width: CGFloat = 260

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        search.placeholderString = "Search tabs…"
        search.delegate = self
        search.target = self
        search.action = #selector(filterChanged)
        search.sendsSearchStringImmediately = true
        search.translatesAutoresizingMaskIntoConstraints = false
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("row"))
        column.resizingMask = .autoresizingMask
        table.addTableColumn(column)
        table.headerView = nil
        table.dataSource = self
        table.delegate = self
        table.rowHeight = 56
        table.intercellSpacing = CGSize(width: 0, height: 2)
        table.selectionHighlightStyle = .regular
        table.backgroundColor = .clear
        table.target = self
        table.action = #selector(rowClicked)
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false
        addSubview(search)
        addSubview(scroll)
        NSLayoutConstraint.activate([
            search.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            search.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            search.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            scroll.topAnchor.constraint(equalTo: search.bottomAnchor, constant: 6),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    func apply(theme: Theme) {
        self.theme = theme
        layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.03).nsColor.cgColor
        table.reloadData()
    }

    func update(rows: [SidebarRow]) {
        allRows = rows
        applyFilter()
    }

    @objc private func filterChanged() {
        applyFilter()
    }

    private func applyFilter() {
        rows = SidebarModel.filter(allRows, query: search.stringValue)
        table.reloadData()
        if let i = rows.firstIndex(where: {
            if case let .session(s) = $0 {
                return s.focused
            }
            return false
        }) {
            table.selectRowIndexes([i], byExtendingSelection: false)
        }
    }

    @objc private func rowClicked() {
        let i = table.clickedRow
        guard rows.indices.contains(i), case let .session(s) = rows[i] else { return }
        onSelect?(s.id)
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        rows.count
    }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        if case .repo = rows[row] {
            return 24
        }
        return 56
    }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        if case .session = rows[row] {
            return true
        }
        return false
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        switch rows[row] {
        case let .repo(name, path):
            let label = NSTextField(labelWithString: name.uppercased())
            label.font = NSFont.systemFont(ofSize: 10, weight: .semibold)
            label.textColor = theme.foreground.nsColor.withAlphaComponent(0.55)
            label.toolTip = path
            let cell = NSView()
            cell.addSubview(label)
            label.frame = CGRect(x: 12, y: 4, width: Self.width - 24, height: 16)
            return cell
        case let .session(s):
            return SidebarSessionCell(session: s, theme: theme)
        }
    }
}

/// One session row: icon with state badge, title, cwd, branch and diff.
@MainActor
final class SidebarSessionCell: NSView {
    init(session s: SidebarSession, theme: Theme) {
        super.init(frame: CGRect(x: 0, y: 0, width: SidebarView.width, height: 56))
        let fg = theme.foreground.nsColor
        let icon = NSTextField(labelWithString: s.agent == nil || s.agent == "generic" ? ">_" : "✶")
        icon.font = NSFont.monospacedSystemFont(ofSize: 13, weight: .bold)
        icon.textColor = fg.withAlphaComponent(0.8)
        icon.alignment = .center
        icon.wantsLayer = true
        icon.layer?.cornerRadius = 14
        icon.layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.1).nsColor.cgColor
        icon.frame = CGRect(x: 10, y: 14, width: 28, height: 28)
        addSubview(icon)
        let badge = NSTextField(labelWithString: s.stateGlyph)
        badge.font = NSFont.systemFont(ofSize: 10)
        let badgeColour: RGBA = switch s.state {
        case "awaiting_input": theme.palette[3]
        case "thinking", "tool_running": theme.palette[5]
        case "crashed": theme.palette[1]
        default: theme.foreground.scaled(0.6)
        }
        badge.textColor = badgeColour.nsColor
        badge.toolTip = s.stateLabel
        badge.frame = CGRect(x: 30, y: 8, width: 14, height: 14)
        addSubview(badge)
        let title = NSTextField(labelWithString: s.title)
        title.font = NSFont.systemFont(ofSize: 12, weight: .medium)
        title.textColor = fg
        title.lineBreakMode = .byTruncatingTail
        title.frame = CGRect(x: 46, y: 34, width: SidebarView.width - 56, height: 16)
        addSubview(title)
        let cwd = NSTextField(labelWithString: s.cwd.map(GitProbe.abbreviated) ?? "")
        cwd.font = NSFont.systemFont(ofSize: 11)
        cwd.textColor = fg.withAlphaComponent(0.6)
        cwd.lineBreakMode = .byTruncatingMiddle
        cwd.frame = CGRect(x: 46, y: 19, width: SidebarView.width - 56, height: 14)
        addSubview(cwd)
        let branch = NSTextField(labelWithString: s.branch.map { "⎇ \($0)" } ?? "")
        branch.font = NSFont.systemFont(ofSize: 11)
        branch.textColor = fg.withAlphaComponent(0.6)
        branch.lineBreakMode = .byTruncatingTail
        branch.frame = CGRect(x: 46, y: 4, width: SidebarView.width - 130, height: 14)
        addSubview(branch)
        if let diff = s.diffText, let d = s.diff {
            let pill = NSTextField(labelWithString: diff)
            pill.font = NSFont.monospacedSystemFont(ofSize: 10, weight: .semibold)
            pill.textColor = d.removed > d.added ? theme.palette[1].nsColor : theme.palette[2].nsColor
            pill.alignment = .center
            pill.wantsLayer = true
            pill.layer?.cornerRadius = 4
            pill.layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.1).nsColor.cgColor
            pill.frame = CGRect(x: SidebarView.width - 82, y: 4, width: 70, height: 15)
            addSubview(pill)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }
}
