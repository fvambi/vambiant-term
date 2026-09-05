// One window = one tab group of pane trees. Tabs are native NSWindow
// tabbing (docs/09 `[window] tab_bar = "native"`), so ⌘T creates a sibling
// window in the same tab bar.

import AppKit

@MainActor
final class TerminalWindowController: NSWindowController, NSWindowDelegate {
    let daemon: DaemonClient
    let renderer: GridRenderer
    private(set) var container: SplitContainer!
    static let tabbingIdentifier = "com.vambiant.term.main"

    init(daemon: DaemonClient, renderer: GridRenderer) {
        self.daemon = daemon
        self.renderer = renderer
        let cell = CGSize(width: renderer.fonts.metrics.width, height: renderer.fonts.metrics.height)
        let content = NSRect(x: 0, y: 0, width: cell.width * 100 + 16, height: cell.height * 30 + 12)
        let window = NSWindow(
            contentRect: content,
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Vambiant Term"
        window.tabbingMode = .preferred
        window.tabbingIdentifier = Self.tabbingIdentifier
        window.backgroundColor = NSColor(
            red: CGFloat(renderer.theme.background.r),
            green: CGFloat(renderer.theme.background.g),
            blue: CGFloat(renderer.theme.background.b),
            alpha: 1
        )
        window.contentResizeIncrements = cell
        super.init(window: window)
        window.delegate = self
        let first = makePane()
        container = SplitContainer(initial: first)
        container.onFocusChange = { [weak self] pane in self?.window?.title = pane.title }
        window.contentView = container
        window.center()
        window.makeFirstResponder(first.view)
        // The view has a size now, so the session can be created at it.
        first.start(cwd: NSHomeDirectory(), cols: max(first.view.cols, 80), rows: max(first.view.rows, 24))
        window.title = first.title
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    private func makePane() -> PaneController {
        let pane = PaneController(daemon: daemon, renderer: renderer)
        pane.view.onAction = { [weak self, weak pane] action in
            guard let self, let pane else { return }
            perform(action, on: pane)
        }
        pane.view.onDisconnected = { [weak self] in
            self?.window?.title = "\(self?.window?.title ?? "") — daemon connection lost"
        }
        return pane
    }

    func perform(_ action: ShellAction, on pane: PaneController) {
        switch action {
        case .newTab:
            (NSApp.delegate as? AppDelegate)?.newWindow(tabbedWith: window)
        case .newWindow:
            (NSApp.delegate as? AppDelegate)?.newWindow(tabbedWith: nil)
        case .splitRight, .splitDown:
            let newPane = makePane()
            container.split(pane, with: newPane, vertical: action == .splitRight)
            container.layoutSubtreeIfNeeded()
            newPane.start(cwd: NSHomeDirectory(), cols: max(newPane.view.cols, 2), rows: max(newPane.view.rows, 1))
        case let .focus(direction):
            if let next = container.neighbour(of: pane, direction: direction) {
                container.focus(next)
            }
        case .zoomPane:
            container.toggleZoom(pane)
        case .closePane, .detach:
            if !container.remove(pane) {
                pane.detach()
                window?.close()
            }
        case .interruptAgent:
            pane.interrupt()
        case .sendPrefix:
            pane.viewer?.send(bytes: [0x02])
        case let .unavailable(what):
            NSLog("not available yet: %@", what)
            NSSound.beep()
        }
    }

    /// ⌘T through the responder chain (NSWindow calls this on its delegate
    /// chain when tabbing is enabled).
    override func newWindowForTab(_ sender: Any?) {
        (NSApp.delegate as? AppDelegate)?.newWindow(tabbedWith: window)
    }

    func windowWillClose(_ notification: Notification) {
        for pane in container.panes {
            pane.detach()
        }
        (NSApp.delegate as? AppDelegate)?.forget(self)
    }
}
