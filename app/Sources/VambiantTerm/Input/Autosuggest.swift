// Ghost-text suggestions for the Warp-mode editor (docs/01 C1.1, history
// first). Given the typed text and history newest-first, the suggestion
// is the rest of the most recent entry that starts with the text.

import Foundation

enum Autosuggest {
    /// The suffix to show after `text`, or nil.
    static func ghost(for text: String, history: [String]) -> String? {
        guard !text.isEmpty, !text.contains("\n") else { return nil }
        for h in history where h.hasPrefix(text) && h.count > text.count {
            return String(h.dropFirst(text.count))
        }
        return nil
    }

    /// The first word of `ghost` (up to and including the following space),
    /// for accept-one-word.
    static func firstWord(of ghost: String) -> String {
        var out = ""
        var seenWord = false
        for ch in ghost {
            if ch == " " {
                if seenWord {
                    out.append(ch)
                    return out
                }
            } else {
                seenWord = true
            }
            out.append(ch)
        }
        return out
    }
}
