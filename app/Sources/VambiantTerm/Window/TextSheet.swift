// A read-only text sheet: a title line, a monospace body, Close (⎋).
// Used for the last egress payload (⌥⌘E, docs/05 §4.2), which must be
// shown exactly as it left, not summarised.

import AppKit

@MainActor
final class TextSheet: NSObject {
    let window: NSWindow
    private let header = NSTextField(labelWithString: "")
    private let text = NSTextView()
    private let close = NSButton(title: "Close (⎋)", target: nil, action: nil)

    override init() {
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 760, height: 520),
            styleMask: [.titled, .resizable],
            backing: .buffered,
            defer: false
        )
        super.init()
        let content = NSView()
        window.contentView = content
        header.font = NSFont.systemFont(ofSize: 12, weight: .semibold)
        header.lineBreakMode = .byTruncatingMiddle
        text.isEditable = false
        text.font = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
        text.textContainerInset = CGSize(width: 8, height: 8)
        text.autoresizingMask = [.width]
        text.isVerticallyResizable = true
        text.textContainer?.widthTracksTextView = true
        let scroll = NSScrollView()
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        close.bezelStyle = .rounded
        close.keyEquivalent = "\u{1b}"
        close.target = self
        close.action = #selector(closeAction(_:))
        for v in [header, scroll, close] {
            v.translatesAutoresizingMaskIntoConstraints = false
            content.addSubview(v)
        }
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            header.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            header.topAnchor.constraint(equalTo: content.topAnchor, constant: 12),
            scroll.leadingAnchor.constraint(equalTo: header.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 8),
            scroll.bottomAnchor.constraint(equalTo: close.topAnchor, constant: -10),
            close.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            close.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -12),
        ])
    }

    var body: String {
        text.string
    }

    func show(title: String, header headerText: String, body: String, over parent: NSWindow) {
        guard window.sheetParent == nil else { return }
        window.title = title
        header.stringValue = headerText
        text.string = body
        parent.beginSheet(window) { _ in }
    }

    @objc private func closeAction(_ sender: Any?) {
        window.sheetParent?.endSheet(window)
    }
}
