// The Warp-mode command editor (ADR-0011): a native text view pinned at the
// bottom of the pane. ↩ submits, ⇧↩ inserts a newline, ⌃C clears, and
// every ⌘ chord the keymap owns still reaches the pane.

import AppKit

@MainActor
final class InputEditorView: NSTextView {
    var onSubmit: ((String) -> Void)?
    var onClear: (() -> Void)?
    /// ⌘ chords are offered to the pane before the text view sees them.
    var onKeyEquivalent: ((NSEvent) -> Bool)?
    var placeholder = "Type a command, or ask the agent" {
        didSet { needsDisplay = true }
    }

    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if event.keyCode == 0x24 || event.keyCode == 0x4C { // ↩ / keypad enter
            if flags.contains(.shift) || flags.contains(.option) {
                insertNewlineIgnoringFieldEditor(nil)
            } else {
                submit()
            }
            return
        }
        if flags.contains(.control), event.charactersIgnoringModifiers == "c" {
            string = ""
            onClear?()
            return
        }
        super.keyDown(with: event)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if onKeyEquivalent?(event) == true {
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    func submit() {
        let text = string
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            onSubmit?("")
            return
        }
        string = ""
        onSubmit?(text)
    }

    /// Lines of text, for sizing the editor between one and six rows.
    var lineCount: Int {
        max(1, min(6, string.split(separator: "\n", omittingEmptySubsequences: false).count))
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard string.isEmpty, let font else { return }
        let attrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: NSColor.tertiaryLabelColor,
        ]
        let origin = CGPoint(x: textContainerInset.width + 5, y: textContainerInset.height)
        NSAttributedString(string: placeholder, attributes: attrs).draw(at: origin)
    }
}
