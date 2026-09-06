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
    }

    let seq: Int64
    let kind: Kind
    /// `marked` (OSC 133/633) or `heuristic`. Anything else is shown as a guess.
    let confidence: String
    let start: UInt64
    let end: UInt64?

    var isCommand: Bool {
        if case .command = kind {
            return true
        }
        return false
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

    /// Rows holding the command's output, if any.
    var outputRows: ClosedRange<UInt64>? {
        guard isCommand, let end, end > start + 1 else { return nil }
        return (start + 1) ... (end - 1)
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

    /// One `{seq, block}` item of `session.blocks`, or a `session.block`
    /// notification (same shape plus `id`).
    static func parse(item: JSONValue) -> Block? {
        guard let seq = item[path: "seq"]?.doubleValue, let b = item[path: "block"] else { return nil }
        return parse(seq: Int64(seq), block: b)
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
    case copyCommand, copyOutput, rerun, explain
}
