// Agent Mode's conversation panel (12 §L): between the grid and the input
// area, with Warp's `ESC for terminal` header. The body is a read-only
// text view of the turns; under the latest answer, each proposed command
// gets a button that stages it into the editor. Nothing here runs.

import AppKit

@MainActor
final class AgentPanel: NSView {
    var conversation = AgentConversation() {
        didSet { render() }
    }

    /// A command to put in the editor, verbatim; the user still presses ↩.
    var onStage: ((String) -> Void)?
    var onClose: (() -> Void)?

    private let title = NSTextField(labelWithString: "Agent")
    private let hint = NSTextField(labelWithString: "ESC for terminal")
    private let close = NSButton(title: "✕", target: nil, action: nil)
    private let scroll = NSScrollView()
    private let text = NSTextView()
    private let stage = NSStackView()
    private var theme = Theme.warpDark
    private var font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
    private var ticker: Timer?
    static let headerHeight: CGFloat = 26
    static let stageHeight: CGFloat = 30

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        title.font = NSFont.systemFont(ofSize: 11, weight: .semibold)
        hint.font = NSFont.systemFont(ofSize: 11)
        close.isBordered = false
        close.font = NSFont.systemFont(ofSize: 11)
        close.target = self
        close.action = #selector(closeAction(_:))
        text.isEditable = false
        text.isSelectable = true
        text.drawsBackground = false
        text.textContainerInset = CGSize(width: 8, height: 6)
        text.autoresizingMask = [.width]
        text.isVerticallyResizable = true
        text.isHorizontallyResizable = false
        text.textContainer?.widthTracksTextView = true
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        stage.orientation = .horizontal
        stage.spacing = 6
        stage.alignment = .centerY
        for v in [title, hint, close, scroll, stage] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            title.centerYAnchor.constraint(equalTo: topAnchor, constant: Self.headerHeight / 2),
            close.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            close.centerYAnchor.constraint(equalTo: title.centerYAnchor),
            hint.trailingAnchor.constraint(equalTo: close.leadingAnchor, constant: -10),
            hint.centerYAnchor.constraint(equalTo: title.centerYAnchor),
            scroll.topAnchor.constraint(equalTo: topAnchor, constant: Self.headerHeight),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: stage.topAnchor),
            stage.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            stage.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -4),
            stage.heightAnchor.constraint(equalToConstant: Self.stageHeight - 8),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    func apply(theme: Theme, font: NSFont) {
        self.theme = theme
        self.font = font
        layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.06).nsColor.cgColor
        title.textColor = theme.foreground.nsColor
        hint.textColor = theme.foreground.nsColor.withAlphaComponent(0.5)
        close.contentTintColor = theme.foreground.nsColor.withAlphaComponent(0.7)
        render()
    }

    @objc private func closeAction(_ sender: Any?) {
        onClose?()
    }

    @objc private func stageAction(_ sender: NSButton) {
        onStage?(sender.toolTip ?? "")
    }

    private func render() {
        text.textStorage?.setAttributedString(Self.attributed(conversation.turns, theme: theme, font: font))
        text.scrollToEndOfDocument(nil)
        title.stringValue = Self.title(for: conversation)
        for v in stage.arrangedSubviews {
            stage.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        if case let .answer(answer) = conversation.turns.last {
            for command in answer.commands.prefix(4) {
                let button = NSButton(title: "Stage: \(Self.short(command))", target: self, action: #selector(stageAction(_:)))
                button.bezelStyle = .rounded
                button.controlSize = .small
                button.font = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
                button.toolTip = command
                stage.addArrangedSubview(button)
            }
        }
        ticker?.invalidate()
        ticker = nil
        if conversation.isThinking {
            ticker = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.render() }
            }
        }
    }

    /// `Agent · claude-strong · ≈$0.01 · 2.1k in / 600 out`
    nonisolated static func title(for conversation: AgentConversation) -> String {
        var parts = ["Agent"]
        if let last = conversation.turns.last(where: {
            if case .answer = $0 {
                true
            } else {
                false
            }
        }),
            case let .answer(answer) = last {
            parts.append(answer.profile)
        } else if case let .thinking(profile, _) = conversation.turns.last {
            parts.append(profile)
        }
        if let cost = conversation.totalCostUSD {
            parts.append("≈$" + String(format: cost < 0.01 ? "%.4f" : "%.2f", cost))
        }
        let tokens = conversation.totalTokens
        if tokens.input + tokens.output > 0 {
            parts.append("\(AgentAnswer.count(tokens.input)) in / \(AgentAnswer.count(tokens.output)) out")
        }
        return parts.joined(separator: " · ")
    }

    static func short(_ command: String) -> String {
        command.count > 40 ? String(command.prefix(38)) + "…" : command
    }

    static func attributed(_ turns: [AgentTurn], theme: Theme, font: NSFont) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let fg = theme.foreground.nsColor
        let dim = fg.withAlphaComponent(0.55)
        let small = NSFont.systemFont(ofSize: max(9, font.pointSize - 2))
        for turn in turns {
            switch turn {
            case let .user(prompt):
                out.append(NSAttributedString(string: "› \(prompt)\n\n", attributes: [
                    .font: NSFont.systemFont(ofSize: font.pointSize, weight: .semibold),
                    .foregroundColor: theme.palette[4].nsColor,
                ]))
            case let .thinking(profile, since):
                let secs = Int(Date().timeIntervalSince(since))
                out.append(NSAttributedString(string: "● Thinking for \(secs)s · \(profile)\n\n", attributes: [
                    .font: small, .foregroundColor: dim,
                ]))
            case let .answer(answer):
                out.append(body(answer.text, theme: theme, font: font))
                out.append(NSAttributedString(string: "\n\(answer.footer)\n\n", attributes: [
                    .font: small, .foregroundColor: dim,
                ]))
            case let .failure(message):
                out.append(NSAttributedString(string: "✗ \(message)\n\n", attributes: [
                    .font: font, .foregroundColor: theme.palette[1].nsColor,
                ]))
            }
        }
        return out
    }

    /// Prose in the UI font, fenced blocks in the terminal font on a tint.
    private static func body(_ text: String, theme: Theme, font: NSFont) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let prose: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: font.pointSize), .foregroundColor: theme.foreground.nsColor,
        ]
        let code: [NSAttributedString.Key: Any] = [
            .font: font, .foregroundColor: theme.foreground.nsColor,
            .backgroundColor: theme.background.mixed(with: theme.foreground, 0.12).nsColor,
        ]
        var inFence = false
        for raw in text.split(separator: "\n", omittingEmptySubsequences: false) {
            let line = String(raw)
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                inFence.toggle()
                continue
            }
            out.append(NSAttributedString(string: line + "\n", attributes: inFence ? code : prose))
        }
        return out
    }
}
