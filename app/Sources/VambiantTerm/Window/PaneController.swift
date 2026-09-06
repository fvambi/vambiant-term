// One daemon session shown in one MetalGridView. Creating the pane
// creates the session; closing the pane detaches — closing never kills
// (docs/09 `[mux] detach_on_close = true`).

import AppKit
import CVambiantTerm

/// Where the shell is, as the daemon's prompt-phase events report it.
enum PromptPhase: Equatable, Sendable {
    /// No mark seen yet (a program without integration, or just started).
    case unknown
    /// Between `A` and `C`: the shell is reading a command line.
    case prompt
    /// Between `C` and the next `A`: a command owns the keyboard.
    case running
}

@MainActor
final class PaneController {
    let daemon: DaemonClient
    let view: MetalGridView
    /// The grid plus Warp's input area; what the window installs.
    let container: PaneView
    private(set) var phase: PromptPhase = .unknown
    private(set) var cwd: String?
    private(set) var branch: String?
    private(set) var session: SessionInfo?
    private(set) var viewer: SessionViewer?
    private(set) var lastError: String?
    private var lastRequestedSize: (UInt16, UInt16) = (0, 0)
    /// Probe hook: every dirty signal, on the viewer's thread.
    let dirtyHook = DirtyHook()
    var blocks = BlockList() {
        didSet { view.blocks = blocks }
    }

    init(daemon: DaemonClient, renderer: GridRenderer) {
        self.daemon = daemon
        view = MetalGridView(renderer: renderer)
        container = PaneView(grid: view)
        view.onResize = { [weak self] cols, rows in self?.resize(cols: cols, rows: rows) }
        view.onBlockAction = { [weak self] action, block in self?.perform(action, on: block) }
        view.onFindChange = { [weak self] state in self?.runFind(state) }
        view.onFindStep = { [weak self] forward in self?.findStep(forward: forward) }
        view.headerProvider = { [weak self] block in
            BlockDecor.header(for: block, cwd: self?.cwd, branch: self?.branch)
        }
        container.input.editor.onSubmit = { [weak self] text in self?.submit(text) }
        container.input.editor.onKeyEquivalent = { [weak self] event in
            self?.view.performKeyEquivalent(with: event) ?? false
        }
        container.input.setHint("⌘↩ for new agent  ·  ⇧↩ newline")
    }

    // MARK: Warp-mode input

    var warpMode: Bool {
        container.warpMode
    }

    /// `[input] mode`, plus the theme and font the input area draws with.
    func setInputMode(warp: Bool) {
        container.warpMode = warp
        container.input.apply(theme: view.renderer.theme, font: view.renderer.nsFont)
        refreshChips()
    }

    /// The editor takes the keyboard while the shell reads a line; the
    /// grid takes it while a command runs (passwords, TUIs, ^C).
    func focus() {
        if warpMode, phase != .running {
            container.window?.makeFirstResponder(container.input.editor)
        } else {
            container.window?.makeFirstResponder(view)
        }
    }

    private var isFocused: Bool {
        guard let responder = container.window?.firstResponder as? NSView else { return false }
        return responder === view || responder.isDescendant(of: container)
    }

    /// The editor's text to the shell; multi-line input goes as one paste.
    func submit(_ text: String) {
        guard let viewer else { return }
        let line = text.replacingOccurrences(of: "\r\n", with: "\n")
        viewer.send(text: line + "\r")
        if !line.isEmpty {
            phase = .running
            container.input.setHint("running — keys go to the command  ·  ⌃C interrupts")
            container.window?.makeFirstResponder(view)
        }
    }

    private func setPhase(_ new: PromptPhase) {
        guard new != phase else { return }
        phase = new
        switch new {
        case .prompt:
            container.input.setHint("⌘↩ for new agent  ·  ⇧↩ newline")
            if isFocused {
                focus()
            }
        case .running:
            container.input.setHint("running — keys go to the command  ·  ⌃C interrupts")
            if isFocused {
                container.window?.makeFirstResponder(view)
            }
        case .unknown:
            break
        }
    }

    private func refreshChips() {
        container.input.setChips(cwd: cwd, branch: branch, theme: view.renderer.theme)
        view.lastSeqReset()
    }

    private func setCwd(_ path: String) {
        cwd = path
        branch = GitProbe.branch(for: path)
        refreshChips()
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
            if let cwd = info.cwd {
                setCwd(cwd)
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

    private var findGeneration = 0

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
            if var block = Block.parse(item: params) {
                block.cwd = cwd
                blocks.append(block)
            }
        case "session.event" where params[path: "event.kind"]?.stringValue == "blocks_degraded":
            blocks.degraded = params[path: "event.reason"]?.stringValue ?? "shell-integration marks are corrupted"
        case "session.event" where params[path: "event.kind"]?.stringValue == "prompt":
            setPhase(.prompt)
        case "session.event" where params[path: "event.kind"]?.stringValue == "command_started":
            setPhase(.running)
        case "session.event" where params[path: "event.kind"]?.stringValue == "pwd":
            if let path = params[path: "event.path"]?.stringValue {
                setCwd(path)
            }
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
