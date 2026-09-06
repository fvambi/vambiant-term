// One window = one tab group of pane trees. Tabs are native NSWindow
// tabbing (docs/09 `[window] tab_bar = "native"`), so ⌘T creates a sibling
// window in the same tab bar.

import AppKit

@MainActor
final class TerminalWindowController: NSWindowController, NSWindowDelegate {
    let daemon: DaemonClient
    let renderer: GridRenderer
    private(set) var container: SplitContainer!
    private let sidebar = SidebarView()
    private let split = NSSplitView()
    private var diffRefresh: Date = .distantPast
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
        window.appearance = Self.appearance(for: renderer.theme)
        super.init(window: window)
        window.delegate = self
        let first = makePane()
        container = SplitContainer(initial: first)
        container.onFocusChange = { [weak self] _ in self?.refreshSidebar() }
        split.isVertical = true
        split.dividerStyle = .thin
        split.autoresizingMask = [.width, .height]
        split.addArrangedSubview(sidebar)
        split.addArrangedSubview(container)
        split.setHoldingPriority(.defaultLow + 1, forSubviewAt: 0)
        sidebar.widthAnchor.constraint(equalToConstant: SidebarView.width).isActive = true
        sidebar.isHidden = true
        sidebar.onSelect = { [weak self] id in
            guard let self, let pane = container.panes.first(where: { ObjectIdentifier($0) == id }) else { return }
            container.focus(pane)
        }
        window.contentView = split
        window.center()
        split.layoutSubtreeIfNeeded()
        first.focus()
        applyTheme()
        // The view has a size now, so the session can be created at it.
        first.start(cwd: NSHomeDirectory(), cols: max(first.view.cols, 80), rows: max(first.view.rows, 24))
        window.title = first.title
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    /// `[mux] detach_on_close`; false kills the sessions with the window.
    var detachOnClose = true

    /// System controls (search field, buttons, menus) follow the theme's
    /// lightness, so a dark theme gets dark chrome.
    static func appearance(for theme: Theme) -> NSAppearance? {
        let b = theme.background
        let dark = (b.r + b.g + b.b) / 3 < 0.5
        return NSAppearance(named: dark ? .darkAqua : .aqua)
    }

    func applyTheme() {
        window?.appearance = Self.appearance(for: renderer.theme)
        window?.backgroundColor = renderer.theme.background.nsColor
        sidebar.apply(theme: renderer.theme)
    }

    @objc func toggleSidebar(_ sender: Any?) {
        sidebar.isHidden.toggle()
        split.adjustSubviews()
        refreshSidebar()
    }

    /// The window title is the focused pane's cwd (Warp's tab title) and
    /// the sidebar lists every pane with its metadata. Diff totals are
    /// probed at most every three seconds per refresh, off the main thread.
    func refreshSidebar() {
        let focused = container.focused
        window?.title = focused?.displayTitle ?? "Vambiant Term"
        guard !sidebar.isHidden else { return }
        let sessions = container.panes.map { p in
            SidebarSession(
                id: ObjectIdentifier(p), name: p.title, cwd: p.cwd, branch: p.branch,
                lastCommand: p.blocks.commands.last?.cmdline, state: p.sessionState, agent: p.agentKind,
                diff: p.diffStats, focused: p === focused
            )
        }
        sidebar.update(rows: SidebarModel.rows(for: sessions, repoRoot: GitProbe.repoRoot(for:)))
        guard Date().timeIntervalSince(diffRefresh) > 3 else { return }
        diffRefresh = Date()
        for pane in container.panes {
            guard let cwd = pane.cwd else { continue }
            GitProbe.diffStats(for: cwd) { [weak self, weak pane] stats in
                DispatchQueue.main.async {
                    MainActor.assumeIsolated {
                        guard let pane, pane.diffStats?.added != stats?.added || pane.diffStats?.removed != stats?.removed
                        else { return }
                        pane.diffStats = stats
                        self?.refreshSidebar()
                    }
                }
            }
        }
    }

    private func makePane() -> PaneController {
        let pane = PaneController(daemon: daemon, renderer: renderer)
        (NSApp.delegate as? AppDelegate)?.configure(pane)
        pane.view.onAction = { [weak self, weak pane] action in
            guard let self, let pane else { return }
            perform(action, on: pane)
        }
        pane.view.onDisconnected = { [weak self] in
            self?.window?.title = "\(self?.window?.title ?? "") — daemon connection lost"
        }
        pane.onChange = { [weak self] in self?.refreshSidebar() }
        return pane
    }

    func perform(_ action: ShellAction, on pane: PaneController) {
        if performBlockAction(action, on: pane) || performAgentAction(action, on: pane) {
            return
        }
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
        case .openSettings:
            (NSApp.delegate as? AppDelegate)?.showSettings(nil)
        case let .scroll(step):
            pane.view.scroll(step)
        case .promptPrevious, .promptNext, .blockSelectPrevious, .blockSelectNext, .blockExtendPrevious,
             .blockExtendNext, .blockTop, .blockBottom, .blockBookmarkPrevious, .blockBookmarkNext,
             .clearScrollback, .block, .findOpen, .findNext, .findPrevious, .stickyHeaderToggle, .sidebarToggle,
             .paletteOpen, .askAgent, .explainLastFailure:
            break // handled above
        case let .unavailable(what):
            NSLog("not available yet: %@", what)
            NSSound.beep()
        }
    }

    /// Agent Mode actions (docs/06 §6). Returns false for the rest.
    private func performAgentAction(_ action: ShellAction, on pane: PaneController) -> Bool {
        switch action {
        case .askAgent: pane.askFromEditor()
        case .explainLastFailure: pane.explainLastFailure()
        default: return false
        }
        return true
    }

    /// Block and scrollback actions (docs/06 §4). Returns false for the rest.
    private func performBlockAction(_ action: ShellAction, on pane: PaneController) -> Bool {
        switch action {
        case .promptPrevious: pane.jumpPrompt(previous: true)
        case .promptNext: pane.jumpPrompt(previous: false)
        case .blockSelectPrevious: pane.selectBlock(previous: true)
        case .blockSelectNext: pane.selectBlock(previous: false)
        case .blockExtendPrevious: pane.view.extendSelection(previous: true)
        case .blockExtendNext: pane.view.extendSelection(previous: false)
        case .blockTop: pane.scrollSelectedBlock(toTop: true)
        case .blockBottom: pane.scrollSelectedBlock(toTop: false)
        case .blockBookmarkPrevious: pane.jumpBookmark(previous: true)
        case .blockBookmarkNext: pane.jumpBookmark(previous: false)
        case .clearScrollback: pane.clearScrollback()
        case .findOpen: pane.view.showFind()
        case .findNext: pane.findStep(forward: true)
        case .findPrevious: pane.findStep(forward: false)
        case .stickyHeaderToggle: pane.view.toggleStickyHeader(nil)
        case .sidebarToggle: toggleSidebar(nil)
        case .paletteOpen: (NSApp.delegate as? AppDelegate)?.showPalette(for: self, pane: pane)
        case let .block(blockAction):
            if let block = pane.view.selectedBlock.flatMap(pane.blocks.command(seq:)) {
                pane.perform(blockAction, on: block)
            } else {
                NSSound.beep()
            }
        default:
            return false
        }
        return true
    }

    /// ⌘T through the responder chain (NSWindow calls this on its delegate
    /// chain when tabbing is enabled).
    override func newWindowForTab(_ sender: Any?) {
        (NSApp.delegate as? AppDelegate)?.newWindow(tabbedWith: window)
    }

    func windowWillClose(_ notification: Notification) {
        for pane in container.panes {
            if detachOnClose {
                pane.detach()
            } else {
                pane.kill()
            }
        }
        (NSApp.delegate as? AppDelegate)?.forget(self)
    }
}
