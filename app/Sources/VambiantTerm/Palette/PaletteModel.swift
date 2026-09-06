// The command palette's data (docs/01 C1.10, Warp's ⌘P): items from four
// sources, Warp's scope prefixes, and fuzzy ranking. Pure and tested; the
// panel only displays what this returns.

import Foundation

enum PaletteScope: Equatable, Sendable {
    case all, actions, sessions, history, files

    /// `actions:`/`a:`, `sessions:`/`s:`, `history:`/`h:`, `files:`/`f:`.
    static func parse(_ query: String) -> (scope: PaletteScope, text: String) {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        let table: [(String, PaletteScope)] = [
            ("actions:", .actions), ("a:", .actions), ("sessions:", .sessions), ("s:", .sessions),
            ("history:", .history), ("h:", .history), ("files:", .files), ("f:", .files),
        ]
        for (prefix, scope) in table where trimmed.lowercased().hasPrefix(prefix) {
            return (scope, String(trimmed.dropFirst(prefix.count)).trimmingCharacters(in: .whitespaces))
        }
        return (.all, trimmed)
    }
}

struct PaletteItem: Equatable, Sendable, Identifiable {
    enum Kind: Equatable, Sendable {
        case action(id: String)
        case session(paneID: ObjectIdentifier)
        case history(command: String)
        case file(path: String)
    }

    let kind: Kind
    let title: String
    /// Chord, cwd, or nothing.
    let detail: String

    var id: String {
        switch kind {
        case let .action(id): "action:\(id)"
        case let .session(p): "session:\(p.hashValue)"
        case let .history(c): "history:\(c)"
        case let .file(p): "file:\(p)"
        }
    }

    var scope: PaletteScope {
        switch kind {
        case .action: .actions
        case .session: .sessions
        case .history: .history
        case .file: .files
        }
    }
}

enum PaletteRanker {
    /// Subsequence match score: higher is better, nil when `needle` is not
    /// a subsequence of `hay`. Consecutive and word-start hits score more.
    static func score(_ needle: String, in hay: String) -> Int? {
        if needle.isEmpty {
            return 0
        }
        let n = Array(needle.lowercased())
        let h = Array(hay.lowercased())
        var score = 0
        var ni = 0
        var lastHit = -2
        for (hi, ch) in h.enumerated() where ni < n.count && ch == n[ni] {
            score += 1
            if hi == lastHit + 1 {
                score += 2
            }
            if hi == 0 || h[hi - 1] == " " || h[hi - 1] == "/" || h[hi - 1] == "." {
                score += 3
            }
            lastHit = hi
            ni += 1
        }
        guard ni == n.count else { return nil }
        // Shorter haystacks with the same hits rank higher.
        return score * 100 - h.count
    }

    /// Items in `scope` (or all) matching `text`, best first; ties keep
    /// input order.
    static func rank(_ items: [PaletteItem], scope: PaletteScope, text: String) -> [PaletteItem] {
        let candidates = items.filter { scope == .all || $0.scope == scope }
        guard !text.isEmpty else { return candidates }
        return candidates.enumerated().compactMap { i, item -> (Int, Int, PaletteItem)? in
            guard let s = score(text, in: item.title) ?? score(text, in: item.detail).map({ $0 - 50 }) else { return nil }
            return (s, i, item)
        }
        .sorted { $0.0 != $1.0 ? $0.0 > $1.0 : $0.1 < $1.1 }
        .map(\.2)
    }
}
