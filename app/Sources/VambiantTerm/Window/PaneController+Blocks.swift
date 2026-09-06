// Block actions on a pane (docs/06 §4): copy, export, re-input, re-run,
// bookmark. Text comes from the daemon by absolute rows; nothing here
// re-parses the grid.

import AppKit
import CVambiantTerm

struct IdParams: Encodable {
    let id: String
}

struct TextParams: Encodable {
    let id: String
    let from: UInt64
    let to: UInt64
    var format: String = "plain"
}

struct BookmarkParams: Encodable {
    let id: String
    let seq: Int64
    let on: Bool
}

struct TextReply: Decodable {
    let text: String
}

extension PaneController {
    /// Actions on the selection; `block` is the one the menu was opened on
    /// and the fallback when nothing else is selected.
    func perform(_ action: BlockAction, on block: Block) {
        let targets = view.selectedBlocks.count > 1 ? blocks.ordered(view.selectedBlocks) : [block]
        switch action {
        case .copyCommand:
            let cmds = targets.compactMap(\.cmdline)
            guard !cmds.isEmpty else {
                NSLog("block %lld has no command line (the shell did not send 633;E)", block.seq)
                NSSound.beep()
                return
            }
            setPasteboard(cmds.joined(separator: "\n"))
        case .copyOutput:
            setPasteboard(targets.map { text(of: $0.outputRows) }.joined(separator: "\n"))
        case .copyBoth:
            // The command line as a prompt would show it, then the output.
            setPasteboard(targets.map { "$ \($0.cmdline ?? "")\n\(text(of: $0.outputRows))" }.joined(separator: "\n\n"))
        case .exportHTML:
            let html = targets.map { text(of: $0.visualRows, format: "html") }.joined(separator: "\n")
            let plain = targets.map { text(of: $0.visualRows) }.joined(separator: "\n")
            let pb = NSPasteboard.general
            pb.clearContents()
            pb.setString(html, forType: .html)
            pb.setString(plain, forType: .string)
        case .reinput, .reinputSudo:
            guard let cmd = block.cmdline else {
                NSSound.beep()
                return
            }
            // Into the prompt, no newline: the user edits or confirms.
            viewer?.send(text: action == .reinputSudo ? "sudo \(cmd)" : cmd)
        case .bookmark:
            guard let session else { return }
            let on = !block.bookmarked
            do {
                try daemon.invoke("session.block.bookmark", params: BookmarkParams(id: session.id, seq: block.seq, on: on))
                blocks.setBookmark(seq: block.seq, on: on)
            } catch {
                NSLog("bookmark for block %lld failed: %@", block.seq, "\(error)")
                NSSound.beep()
            }
        case .menu:
            view.openBlockMenu()
        case .filter:
            showFilter(for: block)
        case .rerun:
            // The user's own earlier command, on their explicit request; not
            // model output, so rule 5 (stage, never execute) does not apply.
            guard let cmd = block.cmdline else {
                NSSound.beep()
                return
            }
            viewer?.send(text: cmd + "\n")
        case .explain:
            NSLog("not available yet: explain block (ai.explain_last_failure, M-AI)")
            NSSound.beep()
        }
    }

    /// Warp's per-block filter (12 §A11), as a panel over the output text:
    /// the grid cannot hide rows without lying about what the shell drew.
    func showFilter(for block: Block) {
        let output = text(of: block.outputRows)
        let panel = BlockFilterPanel(title: block.cmdline ?? "block \(block.seq)", lines: output.components(separatedBy: "\n"))
        panel.present(from: container.window)
    }

    /// Text of absolute rows through the daemon; empty when there are none
    /// or the daemon refuses (logged, never guessed).
    func text(of rows: ClosedRange<UInt64>?, format: String = "plain") -> String {
        guard let session, let rows else { return "" }
        do {
            let reply: TextReply = try daemon.call(
                "session.text",
                params: TextParams(id: session.id, from: rows.lowerBound, to: rows.upperBound, format: format)
            )
            return reply.text
        } catch {
            NSLog("text for rows %llu-%llu failed: %@", rows.lowerBound, rows.upperBound, "\(error)")
            return ""
        }
    }

    func setPasteboard(_ text: String) {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
    }
}
