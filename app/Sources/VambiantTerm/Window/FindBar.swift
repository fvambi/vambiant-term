// The find bar (⌘F): a search field, regex / case / selected-block
// toggles, the match count, and next / previous. It owns no search logic;
// the pane runs `session.find` and hands results back through `state`.

import AppKit

@MainActor
final class FindBar: NSView, NSSearchFieldDelegate {
    let field = NSSearchField()
    private let regex = NSButton(checkboxWithTitle: ".*", target: nil, action: nil)
    private let caseButton = NSButton(checkboxWithTitle: "Aa", target: nil, action: nil)
    private let blockButton = NSButton(checkboxWithTitle: "in block", target: nil, action: nil)
    private let summary = NSTextField(labelWithString: "")
    static let height: CGFloat = 30

    var onChange: ((String, Bool, Bool, Bool) -> Void)?
    var onStep: ((Bool) -> Void)?
    var onClose: (() -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
        field.placeholderString = "Find in scrollback"
        field.delegate = self
        field.sendsSearchStringImmediately = true
        field.sendsWholeSearchString = false
        field.target = self
        field.action = #selector(changed)
        for b in [regex, caseButton, blockButton] {
            b.target = self
            b.action = #selector(changed)
            b.font = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
        }
        regex.toolTip = "Regular expression"
        caseButton.toolTip = "Case sensitive"
        blockButton.toolTip = "Only inside the selected block"
        summary.font = NSFont.systemFont(ofSize: 11)
        summary.textColor = .secondaryLabelColor
        let prev = NSButton(title: "‹", target: self, action: #selector(previous))
        let next = NSButton(title: "›", target: self, action: #selector(next))
        let close = NSButton(title: "✕", target: self, action: #selector(closeBar))
        for b in [prev, next, close] {
            b.bezelStyle = .accessoryBarAction
        }
        let stack = NSStackView(views: [field, regex, caseButton, blockButton, summary, prev, next, close])
        stack.orientation = .horizontal
        stack.spacing = 6
        stack.edgeInsets = NSEdgeInsets(top: 3, left: 8, bottom: 3, right: 8)
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
            field.widthAnchor.constraint(greaterThanOrEqualToConstant: 220),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    var state: FindState {
        get {
            FindState(
                query: field.stringValue, regex: regex.state == .on, caseSensitive: caseButton.state == .on,
                inSelectedBlock: blockButton.state == .on
            )
        }
        set {
            summary.stringValue = newValue.summary
        }
    }

    func focus() {
        window?.makeFirstResponder(field)
        field.selectText(nil)
    }

    @objc private func changed() {
        let s = state
        onChange?(s.query, s.regex, s.caseSensitive, s.inSelectedBlock)
    }

    @objc private func next() {
        onStep?(true)
    }

    @objc private func previous() {
        onStep?(false)
    }

    @objc private func closeBar() {
        onClose?()
    }

    /// ↩ steps forward, ⇧↩ backwards, ⎋ closes.
    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.insertNewline(_:)):
            onStep?(!NSEvent.modifierFlags.contains(.shift))
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            onClose?()
            return true
        default:
            return false
        }
    }
}
