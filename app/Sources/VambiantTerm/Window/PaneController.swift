// One daemon session shown in one MetalGridView. Creating the pane
// creates the session; closing the pane detaches — closing never kills
// (docs/09 `[mux] detach_on_close = true`).

import AppKit
import CVambiantTerm

@MainActor
final class PaneController {
    let daemon: DaemonClient
    let view: MetalGridView
    private(set) var session: SessionInfo?
    private(set) var viewer: SessionViewer?
    private(set) var lastError: String?
    private var lastRequestedSize: (UInt16, UInt16) = (0, 0)
    /// Probe hook: every dirty signal, on the viewer's thread.
    let dirtyHook = DirtyHook()
    private(set) var blocks = BlockList() {
        didSet { view.blocks = blocks }
    }

    init(daemon: DaemonClient, renderer: GridRenderer) {
        self.daemon = daemon
        view = MetalGridView(renderer: renderer)
        view.onResize = { [weak self] cols, rows in self?.resize(cols: cols, rows: rows) }
        view.onBlockAction = { [weak self] action, block in self?.perform(action, on: block) }
        view.onFindChange = { [weak self] state in self?.runFind(state) }
        view.onFindStep = { [weak self] forward in self?.findStep(forward: forward) }
    }

    // MARK: Find

    /// Runs `session.find` off the main thread; stale replies are dropped.
    func runFind(_ state: FindState) {
        guard let session else { return }
        findGeneration += 1
        let generation = findGeneration
        guard !state.query.isEmpty else {
            var cleared = state
            cleared.matches = []
            cleared.current = nil
            view.applyFind(cleared)
            return
        }
        let base = state
        var from: UInt64?
        var to: UInt64?
        if state.inSelectedBlock, let block = view.selectedBlock.flatMap(blocks.command(seq:)) {
            from = block.visualRows.lowerBound
            to = block.visualRows.upperBound
        }
        let params = FindParams(
            id: session.id, query: state.query, regex: state.regex, caseSensitive: state.caseSensitive,
            from: from, to: to, limit: 5000
        )
        let daemon = daemon
        let bottom = view.viewportTop + UInt64(max(1, view.viewportRows)) - 1
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let result: Result<[FindMatch], Error> = Result { try daemon.call("session.find", params: params) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, generation == self.findGeneration else { return }
                    var next = base
                    switch result {
                    case let .success(matches):
                        next.replace(with: matches, viewportBottom: bottom)
                    case let .failure(error):
                        next.matches = []
                        next.current = nil
                        next.error = "\(error)".replacingOccurrences(of: "session.find: ", with: "")
                    }
                    self.view.applyFind(next)
                    if let i = next.current {
                        self.reveal(next.matches[i])
                    }
                }
            }
        }
    }

    func findStep(forward: Bool) {
        var s = view.findState
        guard let m = s.step(forward: forward) else {
            NSSound.beep()
            return
        }
        view.applyFind(s)
        reveal(m)
    }

    /// Scrolls so the match is inside the viewport, centred when it was off-screen.
    private func reveal(_ m: FindMatch) {
        let top = view.viewportTop
        let rows = UInt64(max(1, view.viewportRows))
        if m.row < top || m.row >= top + rows {
            viewer?.scroll(VtScrollTo_Row, n: Int64(m.row > rows / 2 ? m.row - rows / 2 : 0))
        }
    }

    /// Spawns the login shell in `cwd` and attaches, or records why not.
    func start(cwd: String?, cols: UInt16, rows: UInt16) {
        do {
            let info = try daemon.newSession(cols: cols, rows: rows, cwd: cwd)
            attach(to: info)
        } catch {
            lastError = "cannot create a session: \(error)"
            NSLog("%@", lastError!)
        }
    }

    func attach(to info: SessionInfo) {
        session = info
        lastRequestedSize = (view.cols, view.rows)
        do {
            let hook = dirtyHook
            let v = try SessionViewer(socket: daemon.socket, sessionID: info.id) { [weak self] in
                hook.fire()
                DispatchQueue.main.async {
                    MainActor.assumeIsolated { self?.view.markDirty() }
                }
            }
            viewer = v
            view.viewer = v
            if view.cols > 0, (view.cols, view.rows) != (info.cols, info.rows) {
                v.resize(cols: view.cols, rows: view.rows)
            }
            loadBlocks()
        } catch {
            lastError = "cannot attach to \(info.id): \(error)"
            NSLog("%@", lastError!)
        }
    }

    private func resize(cols: UInt16, rows: UInt16) {
        guard let viewer, (cols, rows) != lastRequestedSize else { return }
        lastRequestedSize = (cols, rows)
        viewer.resize(cols: cols, rows: rows)
    }

    /// Sends ^C: "interrupt agent" is the pty's interrupt character.
    func interrupt() {
        viewer?.send(bytes: [0x03])
    }

    func detach() {
        view.viewer = nil
        viewer = nil
    }

    /// `[mux] detach_on_close = false`: the session goes with the window.
    func kill() {
        let id = session?.id
        detach()
        guard let id else { return }
        try? daemon.invoke("session.kill", params: ["id": id])
    }

    var title: String {
        if let session {
            return session.name
        }
        return lastError ?? "starting…"
    }

    // MARK: Blocks

    private struct IdParams: Encodable {
        let id: String
    }

    private struct TextParams: Encodable {
        let id: String
        let from: UInt64
        let to: UInt64
        var format: String = "plain"
    }

    private struct BookmarkParams: Encodable {
        let id: String
        let seq: Int64
        let on: Bool
    }

    private var findGeneration = 0

    private struct TextReply: Decodable {
        let text: String
    }

    /// Blocks persisted so far; live ones arrive through `handle(event:)`.
    func loadBlocks() {
        guard let session else { return }
        do {
            let items: [JSONValue] = try daemon.call("session.blocks", params: IdParams(id: session.id))
            var list = BlockList()
            list.replace(with: items.compactMap(Block.parse(item:)))
            list.degraded = blocks.degraded
            blocks = list
        } catch {
            NSLog("blocks for %@ unavailable: %@", session.id, "\(error)")
        }
    }

    /// Daemon broadcasts; only this pane's session is acted on.
    func handle(event method: String, params: JSONValue) {
        guard let session, params[path: "id"]?.stringValue == session.id else { return }
        switch method {
        case "session.block":
            if let block = Block.parse(item: params) {
                blocks.append(block)
            }
        case "session.event" where params[path: "event.kind"]?.stringValue == "blocks_degraded":
            blocks.degraded = params[path: "event.reason"]?.stringValue ?? "shell-integration marks are corrupted"
        case "session.block_changed":
            if let seq = params[path: "seq"]?.doubleValue, let on = params[path: "bookmarked"]?.boolValue {
                blocks.setBookmark(seq: Int64(seq), on: on)
            }
        default:
            break
        }
    }

    /// ⌥↑ / ⌥↓: the nearest bookmark above or below, selected and shown.
    func jumpBookmark(previous: Bool) {
        let top = view.viewportTop
        let target = previous ? blocks.previousBookmark(before: top) : blocks.nextBookmark(after: top)
        guard let target else {
            NSSound.beep()
            return
        }
        view.selectedBlock = target.seq
        viewer?.scroll(VtScrollTo_Row, n: Int64(target.start))
    }

    /// ⌘⇧↑ / ⌘⇧↓: the top or bottom of the selected block at the top of the grid.
    func scrollSelectedBlock(toTop: Bool) {
        guard let block = view.selectedBlock.flatMap(blocks.command(seq:)) else {
            NSSound.beep()
            return
        }
        let rows = UInt64(max(1, view.viewportRows))
        let target = toTop ? block.start : block.visualRows.upperBound.saturating(minus: rows - 1)
        viewer?.scroll(VtScrollTo_Row, n: Int64(target))
    }

    /// ⌘⇧K: drop the scrollback. Blocks older than the cut no longer map,
    /// so the list is reloaded from the daemon afterwards.
    func clearScrollback() {
        guard let session else { return }
        do {
            try daemon.invoke("session.clear", params: IdParams(id: session.id))
            view.selectedBlock = nil
            loadBlocks()
        } catch {
            NSLog("clear scrollback for %@ failed: %@", session.id, "\(error)")
            NSSound.beep()
        }
    }

    /// ⌘↑ / ⌘↓: put the previous/next command line at the top of the grid.
    func jumpPrompt(previous: Bool) {
        let top = view.viewportTop
        let target = previous ? blocks.previousPromptRow(before: top) : blocks.nextPromptRow(after: top)
        guard let target else {
            NSSound.beep()
            return
        }
        viewer?.scroll(VtScrollTo_Row, n: Int64(target))
    }

    /// ⌘⇧↑ / ⌘⇧↓: move the block selection, scrolling it into view.
    func selectBlock(previous: Bool) {
        guard let block = blocks.neighbour(of: view.selectedBlock, previous: previous) else {
            NSSound.beep()
            return
        }
        view.selectedBlock = block.seq
        let top = view.viewportTop
        let bottom = top + UInt64(max(1, Int(view.rows))) - 1
        if block.start < top || block.start > bottom {
            viewer?.scroll(VtScrollTo_Row, n: Int64(block.start))
        }
    }

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

    /// Text of absolute rows through the daemon; empty when there are none
    /// or the daemon refuses (logged, never guessed).
    private func text(of rows: ClosedRange<UInt64>?, format: String = "plain") -> String {
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

    private func setPasteboard(_ text: String) {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
    }
}

private extension UInt64 {
    func saturating(minus n: UInt64) -> UInt64 {
        self > n ? self - n : 0
    }
}

/// `session.find` request.
private struct FindParams: Encodable {
    let id: String
    let query: String
    let regex: Bool
    let caseSensitive: Bool
    let from: UInt64?
    let to: UInt64?
    let limit: Int
    enum CodingKeys: String, CodingKey {
        case id, query, regex, from, to, limit
        case caseSensitive = "case_sensitive"
    }
}

/// Lock-protected optional observer for dirty signals (latency probe).
final class DirtyHook: @unchecked Sendable {
    private let lock = NSLock()
    private var observer: (@Sendable (CFTimeInterval) -> Void)?

    func set(_ observer: (@Sendable (CFTimeInterval) -> Void)?) {
        lock.lock()
        defer { lock.unlock() }
        self.observer = observer
    }

    func fire() {
        lock.lock()
        let observer = self.observer
        lock.unlock()
        observer?(CACurrentMediaTime())
    }
}
