// The sticky command header (docs/06 §4, Warp parity 12 §A7): when a
// block's command line has scrolled off the top, it stays pinned as a
// strip across the pane. Clicking it scrolls to the block's start.

import AppKit

@MainActor
final class StickyHeaderView: NSView {
    private let label = NSTextField(labelWithString: "")
    private let chip = NSTextField(labelWithString: "")
    var onClick: (() -> Void)?
    /// The block shown, so the pane can avoid redundant updates.
    private(set) var seq: Int64?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        label.lineBreakMode = .byTruncatingTail
        label.maximumNumberOfLines = 1
        chip.font = NSFont.monospacedSystemFont(ofSize: 11, weight: .bold)
        chip.alignment = .right
        addSubview(label)
        addSubview(chip)
        isHidden = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    func show(block: Block, font: NSFont, theme: Theme) {
        seq = block.seq
        label.font = font
        label.stringValue = block.cmdline ?? "(command without a recorded command line)"
        chip.stringValue = BlockDecor.chip(for: block)
        layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.08).nsColor.withAlphaComponent(0.97).cgColor
        label.textColor = theme.foreground.nsColor
        let status = BlockDecor.status(of: block)
        let accent = status == .failed ? theme.palette[1] : (status == .ok ? theme.palette[2] : theme.foreground)
        chip.textColor = accent.nsColor
        isHidden = false
        toolTip = "Scrolled-off command; click to jump to its start"
    }

    func hide() {
        seq = nil
        isHidden = true
    }

    override func layout() {
        super.layout()
        let chipWidth: CGFloat = 90
        label.frame = CGRect(x: 8, y: 0, width: bounds.width - chipWidth - 16, height: bounds.height)
        chip.frame = CGRect(x: bounds.width - chipWidth - 8, y: 0, width: chipWidth, height: bounds.height)
    }

    override func mouseDown(with event: NSEvent) {
        onClick?()
    }
}
