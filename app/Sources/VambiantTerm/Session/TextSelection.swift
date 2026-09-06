// Mouse text selection over the grid (12 §A15): cell coordinates with
// absolute rows, so a selection survives scrolling; stream, word, line
// and rectangle modes; Warp's smart select over the line's text. Pure,
// so slicing and hit rules are tested without a window.

import Foundation

struct GridPoint: Equatable, Sendable, Comparable {
    /// Absolute scrollback row.
    let row: UInt64
    let col: Int

    static func < (a: GridPoint, b: GridPoint) -> Bool {
        a.row != b.row ? a.row < b.row : a.col < b.col
    }
}

/// One highlighted run on a viewport row.
struct SelectionSpan: Equatable, Sendable {
    let row: Int
    let col: Int
    let len: Int
}

struct TextSelection: Equatable, Sendable {
    enum Mode: Equatable, Sendable {
        /// Character stream from anchor to head, wrapping across rows.
        case stream
        /// Whole rows.
        case line
        /// A column-bounded box.
        case rectangle
    }

    var anchor: GridPoint
    var head: GridPoint
    var mode: Mode = .stream

    var start: GridPoint {
        min(anchor, head)
    }

    var end: GridPoint {
        max(anchor, head)
    }

    var rows: ClosedRange<UInt64> {
        start.row ... end.row
    }

    /// A click without a drag selects nothing.
    var isEmpty: Bool {
        mode == .stream && anchor == head
    }

    /// Viewport spans for rows `top ..< top + rows`, clamped to `cols`.
    func spans(top: UInt64, rows: Int, cols: Int) -> [SelectionSpan] {
        guard !isEmpty, cols > 0 else { return [] }
        var out: [SelectionSpan] = []
        for r in 0 ..< rows {
            let abs = top + UInt64(r)
            guard abs >= start.row, abs <= end.row else { continue }
            let (from, to): (Int, Int) = switch mode {
            case .line: (0, cols - 1)
            case .rectangle: (min(anchor.col, head.col), max(anchor.col, head.col))
            case .stream:
                (abs == start.row ? start.col : 0, abs == end.row ? end.col : cols - 1)
            }
            let lo = max(0, min(from, cols - 1))
            let hi = max(lo, min(to, cols - 1))
            out.append(SelectionSpan(row: r, col: lo, len: hi - lo + 1))
        }
        return out
    }

    /// The selected text given the rows' text, one string per absolute
    /// row from `start.row` up. Columns are unicode scalars, like cells.
    func text(lines: [String]) -> String {
        var out: [String] = []
        for (i, line) in lines.enumerated() {
            let abs = start.row + UInt64(i)
            guard abs <= end.row else { break }
            let scalars = Array(line.unicodeScalars)
            let (from, to): (Int, Int?) = switch mode {
            case .line: (0, nil)
            case .rectangle: (min(anchor.col, head.col), max(anchor.col, head.col))
            case .stream: (abs == start.row ? start.col : 0, abs == end.row ? end.col : nil)
            }
            let lo = min(from, scalars.count)
            let hi = to.map { min($0 + 1, scalars.count) } ?? scalars.count
            var piece = String(String.UnicodeScalarView(scalars[lo ..< max(lo, hi)]))
            if mode != .rectangle, to == nil || abs != end.row {
                while piece.last == " " {
                    piece.removeLast()
                }
            }
            out.append(piece)
        }
        return out.joined(separator: "\n")
    }

    /// Warp's smart select: the URL, path, email, address or number under
    /// `col`, else the word (letters, digits, `_`, `-`, `.`).
    static func smartRange(in line: String, at col: Int) -> Range<Int>? {
        let scalars = Array(line.unicodeScalars)
        guard col >= 0, col < scalars.count else { return nil }
        let patterns = [
            #"https?://[^\s'"<>]+"#,
            #"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#,
            #"(?:~|\.{1,2})?/[^\s'"`]+"#,
            #"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b"#,
            #"-?\b\d+(?:\.\d+)?\b"#,
            #"[\p{L}\p{N}_.\-]+"#,
        ]
        let ns = line as NSString
        for p in patterns {
            guard let re = try? NSRegularExpression(pattern: p) else { continue }
            for m in re.matches(in: line, range: NSRange(location: 0, length: ns.length)) {
                guard let r = Range(m.range, in: line) else { continue }
                let lo = line.unicodeScalars.distance(from: line.unicodeScalars.startIndex, to: r.lowerBound)
                let hi = line.unicodeScalars.distance(from: line.unicodeScalars.startIndex, to: r.upperBound)
                if col >= lo, col < hi {
                    return lo ..< hi
                }
            }
        }
        return nil
    }
}
