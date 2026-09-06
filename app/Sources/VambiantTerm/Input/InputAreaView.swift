// Warp's bottom input area (12 §L): a row of context chips (cwd, branch),
// the editor, and a hint line. It is chrome around `InputEditorView`; the
// pane decides when the editor takes the keyboard.

import AppKit

extension RGBA {
    var nsColor: NSColor {
        NSColor(red: CGFloat(r), green: CGFloat(g), blue: CGFloat(b), alpha: 1)
    }
}

@MainActor
final class ChipView: NSView {
    private let label = NSTextField(labelWithString: "")

    init(text: String, tint: NSColor) {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 6
        layer?.backgroundColor = tint.withAlphaComponent(0.16).cgColor
        label.font = NSFont.monospacedSystemFont(ofSize: 11, weight: .semibold)
        label.textColor = tint
        label.stringValue = text
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            label.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            label.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -2),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }
}

@MainActor
final class InputAreaView: NSView {
    let editor = InputEditorView()
    private let chips = NSStackView()
    private let hint = NSTextField(labelWithString: "")
    private let scroll = NSScrollView()
    private var editorHeight: NSLayoutConstraint!
    static let chipRowHeight: CGFloat = 26
    static let hintHeight: CGFloat = 18

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        chips.orientation = .horizontal
        chips.spacing = 6
        chips.alignment = .centerY
        chips.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = editor
        scroll.hasVerticalScroller = false
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false
        editor.isRichText = false
        editor.isAutomaticQuoteSubstitutionEnabled = false
        editor.isAutomaticDashSubstitutionEnabled = false
        editor.isAutomaticTextReplacementEnabled = false
        editor.isAutomaticSpellingCorrectionEnabled = false
        editor.allowsUndo = true
        editor.drawsBackground = false
        editor.textContainerInset = CGSize(width: 4, height: 4)
        editor.autoresizingMask = [.width]
        editor.isVerticallyResizable = true
        editor.isHorizontallyResizable = false
        editor.textContainer?.widthTracksTextView = true
        hint.font = NSFont.systemFont(ofSize: 11)
        hint.translatesAutoresizingMaskIntoConstraints = false
        addSubview(chips)
        addSubview(scroll)
        addSubview(hint)
        editorHeight = scroll.heightAnchor.constraint(equalToConstant: 24)
        NSLayoutConstraint.activate([
            chips.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            chips.topAnchor.constraint(equalTo: topAnchor, constant: 4),
            chips.heightAnchor.constraint(equalToConstant: Self.chipRowHeight - 8),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            scroll.topAnchor.constraint(equalTo: chips.bottomAnchor, constant: 4),
            editorHeight,
            hint.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            hint.topAnchor.constraint(equalTo: scroll.bottomAnchor, constant: 2),
            hint.heightAnchor.constraint(equalToConstant: Self.hintHeight),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    /// Total height for `lines` of editor text.
    static func height(lines: Int, lineHeight: CGFloat) -> CGFloat {
        chipRowHeight + CGFloat(max(1, min(6, lines))) * lineHeight + 8 + hintHeight + 8
    }

    func apply(theme: Theme, font: NSFont) {
        layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.04).nsColor.cgColor
        editor.font = font
        editor.textColor = theme.foreground.nsColor
        editor.tokenColours = [
            .command: theme.foreground.nsColor,
            .flag: theme.palette[6].nsColor,
            .string: theme.palette[3].nsColor,
            .variable: theme.palette[5].nsColor,
            .op: theme.foreground.scaled(0.6).nsColor,
            .comment: theme.foreground.scaled(0.5).nsColor,
        ]
        editor.insertionPointColor = theme.cursor.nsColor
        hint.textColor = theme.foreground.nsColor.withAlphaComponent(0.5)
        editorHeight.constant = CGFloat(editor.lineCount) * font.boundingRectForFont.height + 8
    }

    /// cwd and branch chips, in Warp's order; `nil` branch hides the chip.
    func setChips(cwd: String?, branch: String?, theme: Theme) {
        for v in chips.arrangedSubviews {
            chips.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        let blue = theme.palette[4].nsColor
        let green = theme.palette[2].nsColor
        if let cwd {
            chips.addArrangedSubview(ChipView(text: "📁 \(GitProbe.abbreviated(cwd))", tint: blue))
        }
        if let branch {
            chips.addArrangedSubview(ChipView(text: "⎇ \(branch)", tint: green))
        }
    }

    func setHint(_ text: String) {
        hint.stringValue = text
    }

    func relayout(lineHeight: CGFloat) {
        editorHeight.constant = CGFloat(editor.lineCount) * lineHeight + 8
    }
}
