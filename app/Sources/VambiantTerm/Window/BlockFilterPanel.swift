// Warp's block filter (12 §A11): plain / regex / invert / case-sensitive
// over one block's output, with context lines, in a sheet. Pure filtering
// lives in `BlockFilter` so it is testable.

import AppKit

struct BlockFilter: Equatable, Sendable {
    var query = ""
    var regex = false
    var invert = false
    var caseSensitive = false
    var context = 0

    /// Line indices to show, in order, for `lines`.
    func apply(to lines: [String]) -> [Int] {
        guard !query.isEmpty else { return Array(lines.indices) }
        let matcher: (String) -> Bool
        if regex {
            guard let re = try? NSRegularExpression(pattern: query, options: caseSensitive ? [] : [.caseInsensitive]) else {
                return []
            }
            matcher = { re.firstMatch(in: $0, range: NSRange($0.startIndex..., in: $0)) != nil }
        } else if caseSensitive {
            matcher = { $0.contains(query) }
        } else {
            let q = query.lowercased()
            matcher = { $0.lowercased().contains(q) }
        }
        var keep = Set<Int>()
        for (i, line) in lines.enumerated() where matcher(line) != invert {
            for j in max(0, i - context) ... min(lines.count - 1, i + context) {
                keep.insert(j)
            }
        }
        return keep.sorted()
    }
}

@MainActor
final class BlockFilterPanel: NSObject, NSSearchFieldDelegate {
    private let lines: [String]
    private let window: NSWindow
    private let field = NSSearchField()
    private let regex = NSButton(checkboxWithTitle: ".*", target: nil, action: nil)
    private let invert = NSButton(checkboxWithTitle: "invert", target: nil, action: nil)
    private let caseButton = NSButton(checkboxWithTitle: "Aa", target: nil, action: nil)
    private let context = NSTextField(string: "0")
    private let text = NSTextView()
    private let count = NSTextField(labelWithString: "")

    init(title: String, lines: [String]) {
        self.lines = lines
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 720, height: 460),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false
        )
        super.init()
        window.title = "Filter: \(title)"
        let content = NSView()
        window.contentView = content
        field.placeholderString = "Show lines matching…"
        field.delegate = self
        field.target = self
        field.action = #selector(changed)
        field.sendsSearchStringImmediately = true
        for b in [regex, invert, caseButton] {
            b.target = self
            b.action = #selector(changed)
        }
        context.placeholderString = "ctx"
        context.target = self
        context.action = #selector(changed)
        context.widthAnchor.constraint(equalToConstant: 40).isActive = true
        count.font = NSFont.systemFont(ofSize: 11)
        count.textColor = .secondaryLabelColor
        let close = NSButton(title: "Done", target: self, action: #selector(done))
        close.keyEquivalent = "\u{1b}"
        let bar = NSStackView(views: [field, regex, invert, caseButton, context, count, close])
        bar.orientation = .horizontal
        bar.spacing = 6
        bar.edgeInsets = NSEdgeInsets(top: 8, left: 10, bottom: 4, right: 10)
        bar.translatesAutoresizingMaskIntoConstraints = false
        text.isEditable = false
        text.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        let scroll = NSScrollView()
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.translatesAutoresizingMaskIntoConstraints = false
        text.autoresizingMask = [.width]
        text.isVerticallyResizable = true
        text.textContainer?.widthTracksTextView = true
        content.addSubview(bar)
        content.addSubview(scroll)
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: content.topAnchor),
            bar.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            field.widthAnchor.constraint(greaterThanOrEqualToConstant: 240),
            scroll.topAnchor.constraint(equalTo: bar.bottomAnchor),
            scroll.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: content.bottomAnchor),
        ])
        changed()
    }

    func present(from parent: NSWindow?) {
        if let parent {
            parent.beginSheet(window)
        } else {
            window.makeKeyAndOrderFront(nil)
        }
        window.makeFirstResponder(field)
    }

    @objc private func changed() {
        let filter = BlockFilter(
            query: field.stringValue, regex: regex.state == .on, invert: invert.state == .on,
            caseSensitive: caseButton.state == .on, context: max(0, context.integerValue)
        )
        let shown = filter.apply(to: lines)
        text.string = shown.map { lines[$0] }.joined(separator: "\n")
        count.stringValue = "\(shown.count) of \(lines.count) lines"
    }

    @objc private func done() {
        if let parent = window.sheetParent {
            parent.endSheet(window)
        } else {
            window.close()
        }
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        if selector == #selector(NSResponder.cancelOperation(_:)) {
            done()
            return true
        }
        return false
    }
}
