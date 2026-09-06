// Warp's approval card, in the pane whose agent is waiting (12 §L:
// `Reject ^C · Edit ⌘E · Run ↵` — ours are buttons, never a bare ↩, so an
// Enter meant for the shell cannot approve a tool call). The verdict is
// inline with its token (docs/06 §3); the inbox sheet has the rest.

import AppKit

@MainActor
final class ApprovalCard: NSView {
    var onDecide: ((InboxItem, InboxDecision) -> Void)?
    var onOpenInbox: (() -> Void)?
    /// Warp's `Edit ⌘E`: the inbox sheet with the command field focused.
    var onEdit: (() -> Void)?
    private(set) var item: InboxItem?
    private let title = NSTextField(labelWithString: "")
    private let body = NSTextField(wrappingLabelWithString: "")
    private let verdict = NSTextField(wrappingLabelWithString: "")
    private let why = NSTextField(labelWithString: "")
    private let allow = NSButton(title: "Allow", target: nil, action: nil)
    private let deny = NSButton(title: "Deny", target: nil, action: nil)
    private let edit = NSButton(title: "Edit…", target: nil, action: nil)
    private let more = NSButton(title: "Inbox ⌘⇧A", target: nil, action: nil)
    static let height: CGFloat = 118

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 8
        title.font = NSFont.systemFont(ofSize: 11, weight: .semibold)
        body.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        body.maximumNumberOfLines = 2
        verdict.font = NSFont.systemFont(ofSize: 11)
        verdict.maximumNumberOfLines = 2
        why.font = NSFont.systemFont(ofSize: 10)
        why.lineBreakMode = .byTruncatingTail
        why.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        for (button, action) in [
            (allow, #selector(allowAction(_:))), (deny, #selector(denyAction(_:))), (edit, #selector(editAction(_:))),
            (more, #selector(moreAction(_:))),
        ] {
            button.bezelStyle = .rounded
            button.controlSize = .small
            button.target = self
            button.action = action
        }
        allow.keyEquivalent = ""
        for v in [title, body, verdict, why, allow, deny, edit, more] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            body.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            body.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 4),
            verdict.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            verdict.trailingAnchor.constraint(equalTo: body.trailingAnchor),
            verdict.topAnchor.constraint(equalTo: body.bottomAnchor, constant: 3),
            why.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            why.trailingAnchor.constraint(lessThanOrEqualTo: allow.leadingAnchor, constant: -8),
            why.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8),
            more.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            more.centerYAnchor.constraint(equalTo: why.centerYAnchor),
            edit.trailingAnchor.constraint(equalTo: more.leadingAnchor, constant: -6),
            edit.centerYAnchor.constraint(equalTo: why.centerYAnchor),
            deny.trailingAnchor.constraint(equalTo: edit.leadingAnchor, constant: -6),
            deny.centerYAnchor.constraint(equalTo: why.centerYAnchor),
            allow.trailingAnchor.constraint(equalTo: deny.leadingAnchor, constant: -6),
            allow.centerYAnchor.constraint(equalTo: why.centerYAnchor),
        ])
        isHidden = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    func apply(theme: Theme) {
        layer?.backgroundColor = theme.palette[3].nsColor.withAlphaComponent(0.12).cgColor
        layer?.borderColor = theme.palette[3].nsColor.withAlphaComponent(0.5).cgColor
        layer?.borderWidth = 1
        title.textColor = theme.palette[3].nsColor
        body.textColor = theme.foreground.nsColor
        verdict.textColor = theme.foreground.nsColor.withAlphaComponent(0.85)
        why.textColor = theme.foreground.nsColor.withAlphaComponent(0.55)
    }

    /// Shows `item`, or hides the card for nil.
    func show(_ item: InboxItem?, pending: Int) {
        self.item = item
        guard let item else {
            isHidden = true
            return
        }
        isHidden = false
        let others = pending > 1 ? " · \(pending - 1) more" : ""
        title.stringValue = "✋ \(item.sessionName) asks to run \(item.request.tool) · waiting \(item.waitingLabel)\(others)"
        body.stringValue = item.summary
        verdict.stringValue = [item.verdictLine, item.floorLine].compactMap(\.self).joined(separator: "  ·  ")
        why.stringValue = "asked because \(item.askedBecause)"
        allow.isEnabled = !item.promptShown
        deny.isEnabled = !item.promptShown
        edit.isEnabled = !item.promptShown && item.command != nil
    }

    @objc private func editAction(_ sender: Any?) {
        onEdit?()
    }

    @objc private func allowAction(_ sender: Any?) {
        if let item {
            onDecide?(item, .allow(updatedCommand: nil))
        }
    }

    @objc private func denyAction(_ sender: Any?) {
        if let item {
            onDecide?(item, .deny(reason: "denied from the Vambiant Term inbox"))
        }
    }

    @objc private func moreAction(_ sender: Any?) {
        onOpenInbox?()
    }
}
