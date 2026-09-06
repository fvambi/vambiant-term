// The Warp-mode command editor (ADR-0011): a native text view pinned at the
// bottom of the pane. ↩ submits, ⇧↩ inserts a newline, ⌃C clears, ↑/↓
// walk the daemon's history with the typed prefix, → / ⌃F accept the
// ghost suggestion (⌃→ one word), and every ⌘ chord the keymap owns still
// reaches the pane.

import AppKit

@MainActor
final class InputEditorView: NSTextView {
    var onSubmit: ((String) -> Void)?
    var onClear: (() -> Void)?
    /// ⌘↩: the text goes to the agent, not the shell (ADR-0011 D2).
    var onAgent: ((String) -> Void)?
    /// ⎋ with nothing to cancel in the editor: the pane decides.
    var onEscape: (() -> Void)?
    /// ⌃R: Warp's command search (the palette's history scope).
    var onHistorySearch: (() -> Void)?
    /// ⌘ chords are offered to the pane before the text view sees them.
    var onKeyEquivalent: ((NSEvent) -> Bool)?
    /// History newest-first for a prefix (the pane asks the daemon).
    var historyProvider: ((String) -> [String])?
    var placeholder = "Type a command, or ask the agent" {
        didSet { needsDisplay = true }
    }

    /// Colours for the tokenizer's kinds; set from the theme.
    var tokenColours: [ShellToken: NSColor] = [:]
    /// Executables on PATH (12 §B5); nil until the daemon answered, and
    /// then no underline is a guess.
    var knownCommands: Set<String>? {
        didSet { highlight() }
    }

    var errorColour = NSColor.systemRed
    private(set) var ghost: String?
    /// A correction for the last failed command (12 §B8): shown as ghost
    /// text while the editor is empty, accepted with → / ⌃F, dropped by
    /// typing or ⎋. Never run on its own.
    var correction: String? {
        didSet {
            if string.isEmpty {
                ghost = correction
                needsDisplay = true
            }
        }
    }

    private var historyMatches: [String] = []
    private var historyIndex: Int?
    private var draft = ""

    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        switch event.keyCode {
        case 0x24, 0x4C: // ↩ / keypad enter
            if flags.contains(.command) {
                askAgent()
            } else if flags.contains(.shift) || flags.contains(.option) {
                insertNewlineIgnoringFieldEditor(nil)
            } else {
                submit()
            }
            return
        case 0x35 where correction != nil && string.isEmpty: // ⎋ drops the correction
            correction = nil
            ghost = nil
            needsDisplay = true
            return
        case 0x35 where ghost == nil && historyIndex == nil: // ⎋
            onEscape?()
            return
        case 0x7E where isOnFirstLine: // ↑
            walkHistory(back: true)
            return
        case 0x7D where historyIndex != nil: // ↓
            walkHistory(back: false)
            return
        case 0x7C where caretAtEnd && ghost != nil: // →
            acceptGhost(wordOnly: flags.contains(.control))
            return
        default:
            break
        }
        if flags.contains(.control) {
            switch event.charactersIgnoringModifiers {
            case "c":
                string = ""
                didChangeText()
                onClear?()
                return
            case "f" where caretAtEnd && ghost != nil:
                acceptGhost(wordOnly: false)
                return
            case "r":
                onHistorySearch?()
                return
            default:
                break
            }
        }
        super.keyDown(with: event)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if onKeyEquivalent?(event) == true {
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    /// The whole text to the agent; the editor is cleared like on ↩.
    func askAgent() {
        let text = string
        historyIndex = nil
        string = ""
        didChangeText()
        onAgent?(text)
    }

    func submit() {
        let text = string
        historyIndex = nil
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            onSubmit?("")
            return
        }
        string = ""
        didChangeText()
        onSubmit?(text)
    }

    // MARK: History and suggestions

    private var isOnFirstLine: Bool {
        !(string as NSString).substring(to: selectedRange().location).contains("\n")
    }

    private var caretAtEnd: Bool {
        selectedRange().location == (string as NSString).length && selectedRange().length == 0
    }

    private func walkHistory(back: Bool) {
        if historyIndex == nil {
            draft = string
            historyMatches = historyProvider?(draft) ?? []
        }
        let next = (historyIndex ?? -1) + (back ? 1 : -1)
        if next < 0 {
            historyIndex = nil
            replaceAll(with: draft)
            return
        }
        guard next < historyMatches.count else {
            NSSound.beep()
            return
        }
        historyIndex = next
        replaceAll(with: historyMatches[next])
    }

    private func replaceAll(with text: String) {
        let keep = historyIndex
        string = text
        setSelectedRange(NSRange(location: (text as NSString).length, length: 0))
        didChangeText()
        historyIndex = keep
    }

    private func acceptGhost(wordOnly: Bool) {
        guard let ghost else { return }
        let piece = wordOnly ? Autosuggest.firstWord(of: ghost) : ghost
        insertText(piece, replacementRange: selectedRange())
    }

    override func didChangeText() {
        super.didChangeText()
        historyIndex = nil
        highlight()
        if string.isEmpty {
            ghost = correction
        } else {
            correction = nil
            ghost = Autosuggest.ghost(for: string, history: historyProvider?(string) ?? [])
        }
        needsDisplay = true
    }

    private func highlight() {
        guard let storage = textStorage, let font, let base = textColor else { return }
        let full = NSRange(location: 0, length: storage.length)
        storage.beginEditing()
        storage.setAttributes([.font: font, .foregroundColor: base], range: full)
        let scalars = Array(string.unicodeScalars)
        for span in CommandHighlighter.spans(string) {
            guard let colour = tokenColours[span.kind] else { continue }
            // Scalar offsets → UTF-16 offsets for NSRange.
            let start = String(String.UnicodeScalarView(scalars[0 ..< span.range.lowerBound])).utf16.count
            let length = String(String.UnicodeScalarView(scalars[span.range])).utf16.count
            var attrs: [NSAttributedString.Key: Any] = [.foregroundColor: colour]
            if span.kind == .command {
                attrs[.font] = NSFontManager.shared.convert(font, toHaveTrait: .boldFontMask)
                let word = String(String.UnicodeScalarView(scalars[span.range]))
                if let known = knownCommands, !CommandHighlighter.isKnown(word, known: known) {
                    attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue | NSUnderlineStyle.patternDash.rawValue
                    attrs[.underlineColor] = errorColour
                }
            }
            storage.addAttributes(attrs, range: NSRange(location: start, length: length))
        }
        storage.endEditing()
    }

    /// Lines of text, for sizing the editor between one and six rows.
    var lineCount: Int {
        max(1, min(6, string.split(separator: "\n", omittingEmptySubsequences: false).count))
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard let font else { return }
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.tertiaryLabelColor]
        if string.isEmpty {
            let origin = CGPoint(x: textContainerInset.width + 5, y: textContainerInset.height)
            NSAttributedString(string: placeholder, attributes: attrs).draw(at: origin)
            return
        }
        guard let ghost, let layoutManager, let textContainer else { return }
        let glyphs = layoutManager.numberOfGlyphs
        guard glyphs > 0 else { return }
        let last = layoutManager.boundingRect(forGlyphRange: NSRange(location: glyphs - 1, length: 1), in: textContainer)
        let origin = CGPoint(x: last.maxX + textContainerInset.width, y: last.minY + textContainerInset.height)
        NSAttributedString(string: ghost, attributes: attrs).draw(at: origin)
    }
}
