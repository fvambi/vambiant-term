// A slim strip above the input area for a supervised agent's session
// (12 §G11): the tool call in flight or the last result, and the task
// list's progress. Click opens the event log.

import AppKit

@MainActor
final class AgentActivityView: NSView {
    var onOpenLog: (() -> Void)?
    private let headline = NSTextField(labelWithString: "")
    private let tasks = NSTextField(labelWithString: "")
    static let height: CGFloat = 40

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        headline.font = NSFont.monospacedSystemFont(ofSize: 11, weight: .medium)
        headline.lineBreakMode = .byTruncatingMiddle
        tasks.font = NSFont.systemFont(ofSize: 10)
        tasks.lineBreakMode = .byTruncatingTail
        for v in [headline, tasks] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            headline.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            headline.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            headline.topAnchor.constraint(equalTo: topAnchor, constant: 5),
            tasks.leadingAnchor.constraint(equalTo: headline.leadingAnchor),
            tasks.trailingAnchor.constraint(equalTo: headline.trailingAnchor),
            tasks.topAnchor.constraint(equalTo: headline.bottomAnchor, constant: 1),
        ])
        isHidden = true
        toolTip = "The agent's activity; click for the event log"
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    func apply(theme: Theme) {
        layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.08).nsColor.cgColor
        headline.textColor = theme.foreground.nsColor
        tasks.textColor = theme.foreground.nsColor.withAlphaComponent(0.7)
    }

    func show(_ activity: AgentActivity) {
        guard let line = activity.headline else {
            isHidden = true
            return
        }
        isHidden = false
        headline.stringValue = line
        tasks.stringValue = activity.taskLine ?? ""
    }

    override func mouseDown(with event: NSEvent) {
        onOpenLog?()
    }
}
