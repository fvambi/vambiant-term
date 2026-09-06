// Find over the scrollback: the query options, the matches the daemon
// returned (`session.find`, absolute rows), and the pure arithmetic for
// stepping through them and mapping them onto the visible grid.

import Foundation

struct FindMatch: Equatable, Sendable, Decodable {
    let row: UInt64
    let col: Int
    let len: Int
}

/// A match on a visible row, for the renderer.
struct MatchDecoration: Equatable, Sendable {
    let row: Int
    let col: Int
    let len: Int
    let current: Bool
}

struct FindState: Equatable, Sendable {
    var query = ""
    var regex = false
    var caseSensitive = false
    var inSelectedBlock = false
    var matches: [FindMatch] = []
    var current: Int?
    /// The daemon refused the query (bad regex): shown instead of a count.
    var error: String?

    var isEmpty: Bool {
        query.isEmpty
    }

    /// "3 of 12", "no matches", or the error; empty for an empty query.
    var summary: String {
        if let error {
            return error
        }
        if query.isEmpty {
            return ""
        }
        if matches.isEmpty {
            return "no matches"
        }
        if let current {
            return "\(current + 1) of \(matches.count)"
        }
        return "\(matches.count) matches"
    }

    /// New results: start from the last match at or above the viewport's
    /// bottom row (Warp searches bottom-up), else the last match.
    mutating func replace(with new: [FindMatch], viewportBottom: UInt64) {
        matches = new
        error = nil
        guard !new.isEmpty else {
            current = nil
            return
        }
        current = new.lastIndex { $0.row <= viewportBottom } ?? new.count - 1
    }

    /// Moves to the next or previous match, wrapping; nil without matches.
    @discardableResult
    mutating func step(forward: Bool) -> FindMatch? {
        guard !matches.isEmpty else { return nil }
        let n = matches.count
        let i = current ?? (forward ? n - 1 : 0)
        current = forward ? (i + 1) % n : (i + n - 1) % n
        return matches[current!]
    }

    func visible(top: UInt64, rows: Int) -> [MatchDecoration] {
        guard rows > 0 else { return [] }
        let bottom = top + UInt64(rows) - 1
        return matches.enumerated().compactMap { i, m in
            guard m.row >= top, m.row <= bottom else { return nil }
            return MatchDecoration(row: Int(m.row - top), col: m.col, len: m.len, current: i == current)
        }
    }
}
