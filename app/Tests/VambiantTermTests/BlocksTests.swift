import Foundation
import Testing
@testable import VambiantTerm

struct BlockParsingTests {
    private func json(_ s: String) -> JSONValue {
        // swiftlint:disable:next force_try
        try! JSONDecoder().decode(JSONValue.self, from: Data(s.utf8))
    }

    @Test func parsesTheDaemonsTaggedShape() {
        let item = json(
            #"{"seq": 13, "block": {"confidence": "marked", "start_line": 1, "end_line": 3, "#
                + #""kind": {"kind": "command", "cmdline": "false", "exit": 1}}}"#
        )
        let b = Block.parse(item: item)
        #expect(b == Block(seq: 13, kind: .command(cmdline: "false", exit: 1), confidence: "marked", start: 1, end: 3))
        #expect(b?.failed == true)
        #expect(b?.outputRows == 2 ... 2)
        #expect(b?.visualRows == 1 ... 2)
    }

    @Test func promptsAndUnknownKindsAreHandled() {
        let prompt = Block.parse(item: json(
            #"{"seq": 1, "block": {"confidence": "marked", "start_line": 4, "end_line": 4, "kind": {"kind": "prompt"}}}"#
        ))
        #expect(prompt?.isCommand == false)
        #expect(prompt?.outputRows == nil)
        let odd = Block.parse(item: json(
            #"{"seq": 2, "block": {"confidence": "marked", "start_line": 4, "kind": {"kind": "agent_tool_call"}}}"#
        ))
        #expect(odd == nil, "an unknown kind is skipped, not mis-rendered")
        let live = Block.parse(item: json(
            #"{"id": "s", "seq": 7, "block": {"confidence": "heuristic", "start_line": 9, "end_line": null, "#
                + #""kind": {"kind": "command", "cmdline": null, "exit": null}}}"#
        ))
        #expect(live?.heuristic == true)
        #expect(live?.exit == nil)
        #expect(live?.visualRows == 9 ... 9)
    }
}

struct BlockListTests {
    private func cmd(_ seq: Int64, _ start: UInt64, _ end: UInt64, exit: Int = 0) -> Block {
        Block(seq: seq, kind: .command(cmdline: "c\(seq)", exit: exit), confidence: "marked", start: start, end: end)
    }

    private var list: BlockList {
        var l = BlockList()
        l.replace(with: [
            Block(seq: 1, kind: .prompt, confidence: "marked", start: 0, end: 0),
            cmd(2, 0, 5, exit: 1),
            cmd(4, 5, 9),
            cmd(3, 9, 12), // arrives out of order
        ])
        return l
    }

    @Test func rowsMapToTheNewestBlockOnSharedBoundaries() {
        let l = list
        #expect(l.command(at: 0)?.seq == 2)
        #expect(l.command(at: 4)?.seq == 2)
        #expect(l.command(at: 5)?.seq == 4, "the finish row is the next block's prompt line")
        #expect(l.command(at: 11)?.seq == 3)
        #expect(l.command(at: 12) == nil)
        #expect(l.commands.map(\.seq) == [2, 4, 3], "sorted by start row")
    }

    @Test func promptNavigationUsesCommandStarts() {
        let l = list
        #expect(l.previousPromptRow(before: 9) == 5)
        #expect(l.previousPromptRow(before: 0) == nil)
        #expect(l.nextPromptRow(after: 0) == 5)
        #expect(l.nextPromptRow(after: 9) == nil)
    }

    @Test func selectionNeighboursWrapFromNothing() {
        let l = list
        #expect(l.neighbour(of: nil, previous: true)?.seq == 3)
        #expect(l.neighbour(of: nil, previous: false)?.seq == 2)
        #expect(l.neighbour(of: 4, previous: true)?.seq == 2)
        #expect(l.neighbour(of: 4, previous: false)?.seq == 3)
        #expect(l.neighbour(of: 3, previous: false) == nil)
    }

    @Test func appendIgnoresDuplicateSequenceNumbers() {
        var l = list
        l.append(cmd(4, 5, 9))
        #expect(l.commands.count == 3)
        l.append(cmd(5, 12, 14))
        #expect(l.commands.last?.seq == 5)
    }

    @Test func decorationsClampToTheViewport() {
        let l = list
        // Viewport rows 3..7: block 2 (rows 0..4) is cut at the top, block 4
        // (rows 5..8) is cut at the bottom, block 3 (9..11) is off-screen.
        let d = BlockDecor.decorations(for: l, top: 3, rows: 5, selected: 4)
        #expect(d.count == 2)
        #expect(d[0] == BlockDecoration(
            firstRow: 0, lastRow: 1, status: .failed, selected: false, heuristic: false, startsHere: false, chip: nil
        ))
        #expect(d[1] == BlockDecoration(
            firstRow: 2, lastRow: 4, status: .ok, selected: true, heuristic: false, startsHere: true, chip: "exit 0"
        ))
        #expect(BlockDecor.decorations(for: l, top: 50, rows: 5, selected: nil).isEmpty)
        #expect(BlockDecor.decorations(for: l, top: 0, rows: 0, selected: nil).isEmpty)
    }

    @Test func chipsSayWhatTheyKnow() {
        let guess = Block(seq: 9, kind: .command(cmdline: nil, exit: nil), confidence: "heuristic", start: 0, end: nil)
        #expect(BlockDecor.chip(for: guess) == "≈ exit ?")
        #expect(BlockDecor.status(of: guess) == .unknown)
        #expect(BlockDecor.chip(for: cmd(1, 0, 1, exit: 130)) == "exit 130")
    }
}

struct BlockKeymapTests {
    @Test func blockActionsResolveFromTheDaemonsIds() {
        #expect(ShellAction.from(id: "prompt.next", label: "", milestone: "M5") == .promptNext)
        #expect(ShellAction.from(id: "block.copy_output", label: "", milestone: "M5") == .block(.copyOutput))
        #expect(ShellAction.from(id: "scrollback.page_up", label: "", milestone: "M5") == .scroll(.pageUp))
    }

    @Test func builtInDefaultsBindTheWarpStyleChords() {
        var k = Keymap()
        #expect(k.resolve(KeyChord("up", command: true)) == .action(.promptPrevious))
        #expect(k.resolve(KeyChord("down", command: true, shift: true)) == .action(.blockSelectNext))
        #expect(k.resolve(KeyChord("pageup", shift: true)) == .action(.scroll(.pageUp)))
        #expect(k.resolve(KeyChord("b", control: true)) == .prefixArmed)
        #expect(k.resolve(KeyChord("[")) == .action(.promptPrevious))
    }
}
