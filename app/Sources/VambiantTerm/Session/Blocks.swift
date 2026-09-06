// Command blocks as the daemon reports them (`session.blocks`,
// `session.block`) and the row arithmetic the pane needs: which block a
// row belongs to, where the previous prompt is, which rows hold a
// command's output. Rows are absolute scrollback rows — the same numbering
// as the grid's `top` — so the mapping is pure and testable.

import Foundation

struct Block: Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        case prompt
        case command(cmdline: String?, exit: Int?)
        /// Output at an idle prompt; always a guess (docs/06 §4 `≈`).
        case background
    }

    let seq: Int64
    let kind: Kind
    /// `marked` (OSC 133/633) or `heuristic`. Anything else is shown as a guess.
    let confidence: String
    let start: UInt64
    let end: UInt64?
    /// User bookmark, stored by the daemon.
    var bookmarked: Bool = false
    /// C..D wall-clock time, when the daemon timed it.
    var durationMs: UInt64?
    /// The row output starts on (the `C` mark); the command line occupies
    /// `start ..< outputLine` even when wrapped or multi-line.
    var outputLine: UInt64?
    /// The session's cwd when the block arrived (not stored; nil after a reload).
    var cwd: String?

    var isCommand: Bool {
        if case .command = kind {
            return true
        }
        return false
    }

    var isBackground: Bool {
        kind == .background
    }

    /// Blocks that get chrome: commands and background output.
    var drawsChrome: Bool {
        isCommand || isBackground
    }

    var cmdline: String? {
        if case let .command(c, _) = kind {
            return c
        }
        return nil
    }

    var exit: Int? {
        if case let .command(_, e) = kind {
            return e
        }
        return nil
    }

    var failed: Bool {
        (exit ?? 0) != 0
    }

    var heuristic: Bool {
        confidence != "marked"
    }

    /// The row the finish mark landed on: where the next prompt is printed.
    var lastRow: UInt64 {
        end ?? start
    }

    /// Rows drawn as this block: the command line down to the last row of
    /// output. The finish row belongs to the next prompt.
    var visualRows: ClosedRange<UInt64> {
        start ... max(start, lastRow > start ? lastRow - 1 : start)
    }

    /// Rows of the command line itself.
    var commandRows: ClosedRange<UInt64> {
        guard let outputLine, outputLine > start else { return start ... start }
        return start ... (outputLine - 1)
    }

    /// Rows holding the command's output, if any.
    var outputRows: ClosedRange<UInt64>? {
        guard isCommand, let end else { return nil }
        let first = commandRows.upperBound + 1
        guard end > first else { return nil }
        return first ... (end - 1)
    }

    /// A `block` object as the daemon serialises `vt_blocks::Block`.
    static func parse(seq: Int64, block b: JSONValue) -> Block? {
        guard let start = b[path: "start_line"]?.doubleValue, start >= 0 else { return nil }
        let kind: Kind
        switch b[path: "kind.kind"]?.stringValue {
        case "prompt":
            kind = .prompt
        case "command":
            kind = .command(
                cmdline: b[path: "kind.cmdline"]?.stringValue,
                exit: b[path: "kind.exit"]?.doubleValue.map { Int($0) }
            )
        case "background":
            kind = .background
        default:
            return nil
        }
        return Block(
            seq: seq,
            kind: kind,
            confidence: b[path: "confidence"]?.stringValue ?? "unknown",
            start: UInt64(start),
            end: b[path: "end_line"]?.doubleValue.map { UInt64(max(0, $0)) }
        )
    }

    /// One `{seq, bookmarked?, block}` item of `session.blocks`, or a
    /// `session.block` notification (same shape plus `id`).
    static func parse(item: JSONValue) -> Block? {
        guard let seq = item[path: "seq"]?.doubleValue, let b = item[path: "block"] else { return nil }
        var block = parse(seq: Int64(seq), block: b)
        block?.bookmarked = item[path: "bookmarked"]?.boolValue ?? false
        block?.durationMs = b[path: "duration_ms"]?.doubleValue.map { UInt64(max(0, $0)) }
        block?.outputLine = b[path: "output_line"]?.doubleValue.map { UInt64(max(0, $0)) }
        return block
    }
}

struct BlockList: Equatable, Sendable {
    private(set) var blocks: [Block] = []
    /// Set when the daemon reported corrupted marks: blocks stopped here.
    /// Shown to the user, never hidden (CLAUDE.md rule 4).
    var degraded: String?

    var commands: [Block] {
        blocks.filter(\.isCommand)
    }

    /// Commands and background blocks, in start order.
    var chrome: [Block] {
        blocks.filter(\.drawsChrome)
    }

    mutating func replace(with new: [Block]) {
        blocks = new
        sort()
    }

    /// Ignores a sequence number already present (a live notification that
    /// raced the initial query).
    mutating func append(_ block: Block) {
        guard !blocks.contains(where: { $0.seq == block.seq }) else { return }
        blocks.append(block)
        sort()
    }

    private mutating func sort() {
        blocks.sort { ($0.start, $0.seq) < ($1.start, $1.seq) }
    }

    func command(seq: Int64) -> Block? {
        blocks.first { $0.seq == seq && $0.isCommand }
    }

    /// `(0.027s)`, `(1.2s)`, `(2m 05s)`: Warp's duration format.
    static func durationText(ms: UInt64) -> String {
        if ms < 1000 {
            return String(format: "(%.3fs)", Double(ms) / 1000)
        }
        if ms < 60000 {
            return String(format: "(%.1fs)", Double(ms) / 1000)
        }
        return String(format: "(%dm %02ds)", Int(ms / 60000), Int(ms % 60000 / 1000))
    }

    mutating func setBookmark(seq: Int64, on: Bool) {
        guard let i = blocks.firstIndex(where: { $0.seq == seq }) else { return }
        blocks[i].bookmarked = on
    }

    var bookmarks: [Block] {
        commands.filter(\.bookmarked)
    }

    /// Nearest bookmarked command starting above `top`.
    func previousBookmark(before top: UInt64) -> Block? {
        bookmarks.last { $0.start < top }
    }

    /// Nearest bookmarked command starting below `top`.
    func nextBookmark(after top: UInt64) -> Block? {
        bookmarks.first { $0.start > top }
    }

    /// Sequence numbers of every command between `a` and `b` inclusive, in
    /// start order (a ⇧-click range).
    func range(from a: Int64, to b: Int64) -> Set<Int64> {
        let cmds = commands
        guard let i = cmds.firstIndex(where: { $0.seq == a }),
              let j = cmds.firstIndex(where: { $0.seq == b })
        else { return [] }
        return Set(cmds[min(i, j) ... max(i, j)].map(\.seq))
    }

    /// Commands in `seqs`, in start order.
    func ordered(_ seqs: Set<Int64>) -> [Block] {
        commands.filter { seqs.contains($0.seq) }
    }

    /// The command block drawn on `row`. Where one block's finish row is
    /// the next block's prompt line, the newer block wins.
    func command(at row: UInt64) -> Block? {
        commands.last { $0.visualRows.contains(row) }
    }

    /// Start row of the nearest command that begins above `top`.
    func previousPromptRow(before top: UInt64) -> UInt64? {
        commands.last { $0.start < top }?.start
    }

    /// Start row of the nearest command that begins below `top`.
    func nextPromptRow(after top: UInt64) -> UInt64? {
        commands.first { $0.start > top }?.start
    }

    /// The command before/after `seq`; with nothing selected, the last
    /// (previous) or first (next) command.
    func neighbour(of seq: Int64?, previous: Bool) -> Block? {
        let cmds = commands
        guard let seq, let i = cmds.firstIndex(where: { $0.seq == seq }) else {
            return previous ? cmds.last : cmds.first
        }
        let j = previous ? i - 1 : i + 1
        return cmds.indices.contains(j) ? cmds[j] : nil
    }
}

/// What the user can do with one block (docs/06 §4's kebab menu).
enum BlockAction: Equatable, Sendable {
    case copyCommand, copyOutput, copyBoth, exportHTML
    case reinput, reinputSudo, rerun
    case bookmark, menu, filter, explain
}
