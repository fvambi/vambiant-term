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
    }

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
        default:
            break
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

    func perform(_ action: BlockAction, on block: Block) {
        switch action {
        case .copyCommand:
            guard let cmd = block.cmdline else {
                NSLog("block %lld has no command line (the shell did not send 633;E)", block.seq)
                NSSound.beep()
                return
            }
            setPasteboard(cmd)
        case .copyOutput:
            guard let session, let rows = block.outputRows else {
                setPasteboard("")
                return
            }
            do {
                let reply: TextReply = try daemon.call(
                    "session.text", params: TextParams(id: session.id, from: rows.lowerBound, to: rows.upperBound)
                )
                setPasteboard(reply.text)
            } catch {
                NSLog("copy output for block %lld failed: %@", block.seq, "\(error)")
                NSSound.beep()
            }
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

    private func setPasteboard(_ text: String) {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
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
