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

    init(daemon: DaemonClient, renderer: GridRenderer) {
        self.daemon = daemon
        view = MetalGridView(renderer: renderer)
        view.onResize = { [weak self] cols, rows in self?.resize(cols: cols, rows: rows) }
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

    var title: String {
        if let session {
            return session.name
        }
        return lastError ?? "starting…"
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
