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
    /// Warp mode: the context line drawn into the blank prompt row above the
    /// command (`firstRow - 1`), and the command rows set in bold.
    var header: String?
    /// Visible rows of the command line (bold in Warp mode).
    var commandRows: ClosedRange<Int>?
}

/// What the renderer draws around blocks, from `[blocks]` in config.
struct BlockChrome: Equatable, Sendable {
    var dividers = true
    var failedTint = true
    var stickyHeader = true
    /// The shell's prompt is a blank row the app fills (ADR-0011).
    var warpMode = true
}

enum BlockDecor {
    /// Decorations for the viewport whose top row is absolute `top`.
    static func decorations(for list: BlockList, top: UInt64, rows: Int, selected: Int64?) -> [BlockDecoration] {
        decorations(for: list, top: top, rows: rows, selected: selected.map { [$0] } ?? [])
    }

    static func decorations(for list: BlockList, top: UInt64, rows: Int, selected: Set<Int64>) -> [BlockDecoration] {
        decorations(for: list, top: top, rows: rows, selected: selected, headers: nil)
    }

    /// `headers(block)` supplies Warp's context line for a command block
    /// whose prompt row is visible; nil disables headers (classic mode).
    static func decorations(
        for list: BlockList, top: UInt64, rows: Int, selected: Set<Int64>, headers: ((Block) -> String)?
    ) -> [BlockDecoration] {
        guard rows > 0 else { return [] }
        let bottom = top + UInt64(rows) - 1
        var out: [BlockDecoration] = []
        for b in list.chrome {
            let span = b.visualRows
            if span.upperBound < top || span.lowerBound > bottom {
                continue
            }
            let startsHere = span.lowerBound >= top
            // The header needs the prompt row (start - 1) on screen too.
            let headerVisible = startsHere && b.isCommand && span.lowerBound > top
            let cmd = b.commandRows
            let commandVisible: ClosedRange<Int>? = b.isCommand && cmd.upperBound >= top && cmd.lowerBound <= bottom
                ? Int(max(cmd.lowerBound, top) - top) ... Int(min(cmd.upperBound, bottom) - top)
                : nil
            out.append(BlockDecoration(
                firstRow: Int(max(span.lowerBound, top) - top),
                lastRow: Int(min(span.upperBound, bottom) - top),
                status: status(of: b),
                selected: selected.contains(b.seq),
                heuristic: b.heuristic,
                startsHere: startsHere,
                chip: startsHere ? chip(for: b) : nil,
                bookmarked: b.bookmarked,
                header: headerVisible ? headers?(b) : nil,
                commandRows: headers != nil ? commandVisible : nil
            ))
        }
        return out
    }

    /// Warp's context line: `~/code/app  git:(main)  (0.027s)`.
    static func header(for b: Block, cwd: String?, branch: String?) -> String {
        var parts: [String] = []
        if let cwd = b.cwd ?? cwd {
            parts.append(GitProbe.abbreviated(cwd))
        }
        if let branch {
            parts.append("git:(\(branch))")
        }
        if let ms = b.durationMs {
            parts.append(BlockList.durationText(ms: ms))
        }
        return parts.joined(separator: "  ")
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
