// Block chrome for one frame: which visible rows carry a gutter stripe,
// where a separator goes, what the exit chip says. Pure so it is testable
// without a GPU; the renderer only draws what it is handed.

import Foundation

struct BlockDecoration: Equatable, Sendable {
    enum Status: Equatable, Sendable {
        case ok, failed, unknown
    }

    /// Visible row indices, clamped to the grid.
    let firstRow: Int
    let lastRow: Int
    let status: Status
    let selected: Bool
    let heuristic: Bool
    /// The block's command line is on `firstRow` (not scrolled off above),
    /// so the separator and the chip are drawn there.
    let startsHere: Bool
    /// Exit chip text for the header row; nil when the header is off-screen.
    let chip: String?
    /// The block is bookmarked (tick in the right gutter on its header row).
    var bookmarked: Bool = false
}

/// What the renderer draws around blocks, from `[blocks]` in config.
struct BlockChrome: Equatable, Sendable {
    var dividers = true
    var failedTint = true
    var stickyHeader = true
}

enum BlockDecor {
    /// Decorations for the viewport whose top row is absolute `top`.
    static func decorations(for list: BlockList, top: UInt64, rows: Int, selected: Int64?) -> [BlockDecoration] {
        decorations(for: list, top: top, rows: rows, selected: selected.map { [$0] } ?? [])
    }

    static func decorations(for list: BlockList, top: UInt64, rows: Int, selected: Set<Int64>) -> [BlockDecoration] {
        guard rows > 0 else { return [] }
        let bottom = top + UInt64(rows) - 1
        var out: [BlockDecoration] = []
        for b in list.chrome {
            let span = b.visualRows
            if span.upperBound < top || span.lowerBound > bottom {
                continue
            }
            let startsHere = span.lowerBound >= top
            out.append(BlockDecoration(
                firstRow: Int(max(span.lowerBound, top) - top),
                lastRow: Int(min(span.upperBound, bottom) - top),
                status: status(of: b),
                selected: selected.contains(b.seq),
                heuristic: b.heuristic,
                startsHere: startsHere,
                chip: startsHere ? chip(for: b) : nil,
                bookmarked: b.bookmarked
            ))
        }
        return out
    }

    static func status(of b: Block) -> BlockDecoration.Status {
        guard b.exit != nil else { return .unknown }
        return b.failed ? .failed : .ok
    }

    /// `exit 0` / `exit 1` / `exit ?`, with docs/06's `≈` when the block is
    /// a guess rather than a shell mark; background output is always one.
    static func chip(for b: Block) -> String {
        if b.isBackground {
            return "≈ background"
        }
        let code = b.exit.map(String.init) ?? "?"
        return (b.heuristic ? "≈ " : "") + "exit \(code)"
    }
}
