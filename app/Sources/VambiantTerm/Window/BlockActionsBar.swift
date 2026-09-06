// Warp's hover toolbar for a block (12 §L): bookmark, export, filter and
// the kebab menu, shown at the top right of the block under the mouse.
// Every button routes to the same block actions the menu offers.

import AppKit

@MainActor
final class BlockActionsBar: NSView {
    var onAction: ((BlockAction) -> Void)?
    private let bookmark = NSButton()
    private var buttons: [NSButton] = []
    static let size = CGSize(width: 116, height: 22)

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 5
        let specs: [(String, String, BlockAction)] = [
            ("bookmark", "Bookmark", .bookmark),
            ("square.and.arrow.up", "Copy as HTML", .exportHTML),
            ("line.3.horizontal.decrease", "Filter output", .filter),
            ("ellipsis", "More", .menu),
        ]
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 2
        stack.translatesAutoresizingMaskIntoConstraints = false
        for (i, (symbol, tip, action)) in specs.enumerated() {
            let b = i == 0 ? bookmark : NSButton()
            b.bezelStyle = .accessoryBarAction
            b.isBordered = false
            b.image = NSImage(systemSymbolName: symbol, accessibilityDescription: tip)
            b.imagePosition = .imageOnly
            b.toolTip = tip
            b.target = self
            b.action = #selector(tapped(_:))
            b.tag = i
            b.widthAnchor.constraint(equalToConstant: 26).isActive = true
            stack.addArrangedSubview(b)
            buttons.append(b)
            actions.append(action)
        }
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
        isHidden = true
    }

    private var actions: [BlockAction] = []

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    func apply(theme: Theme) {
        layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.12).nsColor.cgColor
        for b in buttons {
            b.contentTintColor = theme.foreground.nsColor.withAlphaComponent(0.85)
        }
    }

    func show(for block: Block) {
        bookmark.image = NSImage(
            systemSymbolName: block.bookmarked ? "bookmark.fill" : "bookmark",
            accessibilityDescription: block.bookmarked ? "Remove bookmark" : "Bookmark"
        )
        isHidden = false
    }

    @objc private func tapped(_ sender: NSButton) {
        guard actions.indices.contains(sender.tag) else { return }
        onAction?(actions[sender.tag])
    }
}
