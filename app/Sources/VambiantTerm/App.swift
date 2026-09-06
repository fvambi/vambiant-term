// Entry point. `@main` on a type (not main.swift) keeps the module
// importable from the test target.

import AppKit
import CVambiantTerm

@main
enum VambiantTermApp {
    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private(set) var daemon: DaemonClient!
    private(set) var renderer: GridRenderer!
    private var windows: [TerminalWindowController] = []
    private var probe: LatencyProbe?
    private(set) var configModel: ConfigModel!
    private var settings: SettingsWindowController?
    private var events: EventStream?
    private var shellConfig: ShellConfig?
    private(set) var keymap = Keymap()
    private var appearanceObservation: NSKeyValueObservation?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let abi = vt_ffi_abi_version()
        guard abi == UInt32(VtABI_VERSION) else {
            fatal("libvambiant_term ABI \(abi) does not match the header's \(VtABI_VERSION); rebuild with `mise run ffi:staticlib`")
        }
        let socket = ProcessInfo.processInfo.environment["VAMBIANT_TERM_SOCKET"] ?? DaemonClient.defaultSocket()
        daemon = DaemonClient(socket: socket)
        do {
            _ = try daemon.ensureRunning()
        } catch {
            fatal("\(error)")
        }
        guard let device = MTLCreateSystemDefaultDevice() else { fatal("no Metal device") }
        do {
            renderer = try GridRenderer(
                device: device,
                fonts: FontSet(),
                theme: .warpDark,
                scale: NSScreen.main?.backingScaleFactor ?? 2
            )
        } catch {
            fatal("\(error)")
        }
        configModel = ConfigModel(daemon: daemon)
        configModel.onChange = { [weak self] snapshot in self?.apply(snapshot) }
        configModel.reload()
        do {
            events = try EventStream(socket: socket) { [weak self] method, params in
                self?.route(event: method, params: params)
            }
        } catch {
            NSLog("event stream unavailable: \(error); config changes made outside the app will not be picked up")
        }
        appearanceObservation = NSApp.observe(\.effectiveAppearance) { [weak self] _, _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.configModel.reload() }
            }
        }
        installMenu()
        newWindow(tabbedWith: nil)
        NSApp.activate(ignoringOtherApps: true)
        if let shot = ProcessInfo.processInfo.environment["VAMBIANT_TERM_SCREENSHOT_SETTINGS"] {
            showSettings(nil)
            Timer.scheduledTimer(withTimeInterval: 2.5, repeats: false) { _ in
                MainActor.assumeIsolated {
                    self.settings?.capture(to: shot)
                    exit(0)
                }
            }
        }
        if let n = ProcessInfo.processInfo.environment["VAMBIANT_TERM_LATENCY_PROBE"].flatMap(Int.init),
           let pane = windows.first?.container.focused {
            probe = LatencyProbe(pane: pane, keystrokes: n)
            probe?.start()
        }
        if let shot = ProcessInfo.processInfo.environment["VAMBIANT_TERM_SCREENSHOT"],
           let pane = windows.first?.container.focused {
            runScreenshotDiagnostic(pane: pane, shot: shot)
        }
    }

    /// `VAMBIANT_TERM_SCREENSHOT=<png>`: drive one pane through attributes,
    /// a failing command, block selection, a bookmark, a find and a scroll,
    /// then write the frame and log the overlay state.
    private func runScreenshotDiagnostic(pane: PaneController, shot: String) {
        // Something worth looking at: attributes, colours, CJK, emoji.
        let demo = "printf '\\e[1mbold\\e[0m \\e[3mitalic\\e[0m \\e[4munderline\\e[0m "
            + "\\e[31mred\\e[0m \\e[42m bg \\e[0m \\e[38;5;208m256\\e[0m \\e[38;2;122;162;247mrgb\\e[0m "
            + "世界 😀\\n'; ls -la | head -6; seq 1 40\r"
        Timer.scheduledTimer(withTimeInterval: 1.5, repeats: false) { _ in
            MainActor.assumeIsolated { _ = pane.viewer?.send(text: demo) }
        }
        // A failing command too, so the shot shows both block chips, and
        // the last block selected so the tint and gutter are visible.
        Timer.scheduledTimer(withTimeInterval: 2.5, repeats: false) { _ in
            MainActor.assumeIsolated { _ = pane.viewer?.send(text: "grep -c vambiant /nonexistent\r") }
        }
        // Select the last block, bookmark the first, extend the selection
        // upwards: tint, tick and range are all in the shot.
        Timer.scheduledTimer(withTimeInterval: 3.3, repeats: false) { _ in
            MainActor.assumeIsolated {
                pane.selectBlock(previous: true)
                if let first = pane.blocks.commands.first {
                    pane.perform(.bookmark, on: first)
                }
                pane.view.extendSelection(previous: true)
            }
        }
        // Scroll the first command line off the top and search, so the
        // sticky header and the find highlights are in the shot too.
        Timer.scheduledTimer(withTimeInterval: 3.4, repeats: false) { _ in
            MainActor.assumeIsolated {
                pane.view.selectedBlock = nil
                _ = pane.viewer?.scroll(VtScrollTo_Lines, n: 2)
                pane.view.showFind()
                pane.view.findBar.field.stringValue = "fwartner"
                pane.runFind(FindState(query: "fwartner"))
            }
        }
        // Below the first command line, so its header is pinned for the
        // log line; then back to the top so the shot shows the block's
        // context row and bold command line.
        Timer.scheduledTimer(withTimeInterval: 3.7, repeats: false) { _ in
            MainActor.assumeIsolated { _ = pane.viewer?.scroll(VtScrollTo_Row, n: 10) }
        }
        Timer.scheduledTimer(withTimeInterval: 3.85, repeats: false) { _ in
            MainActor.assumeIsolated {
                let sticky = pane.view.stickyHeader.isHidden ? "hidden" : "block \(pane.view.stickyHeader.seq ?? -1)"
                NSLog("screenshot: sticky header at row 10: %@", sticky)
                _ = pane.viewer?.scroll(VtScrollTo_Top)
            }
        }
        Timer.scheduledTimer(withTimeInterval: 3.9, repeats: false) { _ in
            MainActor.assumeIsolated {
                // The overlays are AppKit views the Metal capture cannot
                // see; log their state so a run still verifies them.
                let sticky = pane.view.stickyHeader.isHidden ? "hidden" : "block \(pane.view.stickyHeader.seq ?? -1)"
                let top = pane.view.viewportTop
                NSLog(
                    "screenshot: find '%@' -> %@; sticky header %@ (top %llu, block at top %lld, chrome %d)",
                    pane.view.findState.query, pane.view.findState.summary, sticky, top,
                    pane.blocks.command(at: top)?.seq ?? -1, pane.view.renderer.blockChrome.stickyHeader ? 1 : 0
                )
                // Typed text in the editor: history ghost text and token colours.
                pane.container.input.editor.string = "grep -c"
                pane.container.input.editor.didChangeText()
                // Hover the second block so its action bar is in the shot.
                if let hovered = pane.blocks.commands.first, hovered.start >= pane.view.viewportTop {
                    let scale = pane.view.window?.backingScaleFactor ?? 2
                    let row = CGFloat(hovered.start - pane.view.viewportTop)
                    pane.view.updateHover(at: CGPoint(
                        x: 40, y: pane.view.padding.height + (row + 0.5) * pane.view.renderer.cellSize.height / scale
                    ))
                }
                NSLog("screenshot: hover block %lld, bar hidden %d", pane.view.hoverBlock ?? -1, pane.view.actionsBar.isHidden ? 1 : 0)
                pane.view.captureNext(to: shot)
                // The whole window (chrome, chips, editor) through AppKit's
                // display cache; the Metal grid inside may come out blank.
                if let content = pane.container.window?.contentView,
                   let rep = content.bitmapImageRepForCachingDisplay(in: content.bounds) {
                    content.cacheDisplay(in: content.bounds, to: rep)
                    if let png = rep.representation(using: .png, properties: [:]) {
                        try? png.write(to: URL(fileURLWithPath: shot + ".window.png"))
                    }
                }
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    @discardableResult
    func newWindow(tabbedWith existing: NSWindow?) -> TerminalWindowController {
        let controller = TerminalWindowController(daemon: daemon, renderer: renderer)
        windows.append(controller)
        if let existing, let w = controller.window {
            existing.addTabbedWindow(w, ordered: .above)
        }
        controller.showWindow(nil)
        controller.window?.makeKeyAndOrderFront(nil)
        return controller
    }

    func forget(_ controller: TerminalWindowController) {
        windows.removeAll { $0 === controller }
    }

    @objc func showSettings(_ sender: Any?) {
        if settings == nil {
            settings = SettingsWindowController(model: configModel)
        }
        settings?.showWindow(nil)
        settings?.window?.makeKeyAndOrderFront(nil)
    }

    /// Every pane view gets the current keymap, padding and cursor settings.
    func configure(_ view: MetalGridView) {
        view.keymap = keymap
        if let c = shellConfig {
            view.padding = CGSize(width: c.window.padding.x, height: c.window.padding.y)
            view.blink = (c.cursor.blink, c.cursor.blinkIntervalMs)
        }
    }

    /// A pane: its grid plus the input mode (`[input] mode`, ADR-0011).
    func configure(_ pane: PaneController) {
        configure(pane.view)
        pane.setInputMode(warp: shellConfig.map { $0.input.mode == "warp" } ?? true)
    }

    private var allPanes: [PaneController] {
        windows.flatMap(\.container.panes)
    }

    private var allViews: [MetalGridView] {
        windows.flatMap { $0.container.panes.map(\.view) }
    }

    /// One daemon broadcast: config reloads here, session-scoped ones go
    /// to every pane (each checks the session id).
    private func route(event method: String, params: String) {
        if method == "config.changed" {
            configModel.reload()
            return
        }
        guard method == "session.block" || method == "session.event",
              let data = params.data(using: .utf8),
              let json = try? JSONDecoder().decode(JSONValue.self, from: data)
        else { return }
        for pane in windows.flatMap(\.container.panes) {
            pane.handle(event: method, params: json)
        }
    }

    /// Applies what the shell honours (docs/09 `applied: now`): fonts,
    /// theme (following the system appearance), padding, cursor, keymap,
    /// close policy. Anything else is the daemon's.
    private func apply(_ snapshot: ConfigSnapshot) {
        keymap = Keymap(resolved: snapshot.keymap, actions: snapshot.actions)
        guard let c = try? snapshot.config.decode(ShellConfig.self) else {
            NSLog("config: cannot decode the shell's keys; keeping the previous values")
            return
        }
        let fontChanged = shellConfig?.font != c.font
        shellConfig = c
        if fontChanged {
            let families = [c.font.family] + c.font.fallback
            do {
                try renderer.setFonts(FontSet(families: families, size: c.font.size, lineHeight: c.font.lineHeight))
            } catch {
                NSLog("config: font change failed: \(error)")
            }
        }
        renderer.boldIsBright = c.font.boldIsBright
        renderer.cursorStyle = CursorStyle(rawValue: c.cursor.style) ?? .block
        renderer.blockChrome = BlockChrome(
            dividers: c.blocks.dividers, failedTint: c.blocks.failedTint, stickyHeader: c.blocks.stickyHeader,
            warpMode: c.input.mode == "warp"
        )
        let dark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let wanted = c.theme.followSystem && !dark ? c.theme.light : c.theme.name
        if let file = snapshot.themes[wanted], let theme = Theme(file: file) {
            renderer.theme = theme
        } else {
            NSLog("config: theme %@ is unavailable; keeping %@", wanted, renderer.theme.name)
        }
        for w in windows {
            w.detachOnClose = c.mux.detachOnClose
            w.applyTheme()
        }
        for pane in allPanes {
            configure(pane)
            pane.view.configChanged()
        }
    }

    @objc func newWindowAction(_ sender: Any?) {
        newWindow(tabbedWith: nil)
    }

    @objc func newTabAction(_ sender: Any?) {
        newWindow(tabbedWith: NSApp.keyWindow)
    }

    private func installMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        let appMenu = NSMenu()
        appMenu.addItem(
            withTitle: "About Vambiant Term",
            action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)),
            keyEquivalent: ""
        )
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Settings…", action: #selector(showSettings(_:)), keyEquivalent: ",")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit Vambiant Term", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        main.addItem(appItem)

        let shell = NSMenuItem()
        let shellMenu = NSMenu(title: "Shell")
        shellMenu.addItem(withTitle: "New Window", action: #selector(newWindowAction(_:)), keyEquivalent: "n")
        shellMenu.addItem(withTitle: "New Tab", action: #selector(newTabAction(_:)), keyEquivalent: "t")
        shellMenu.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        shell.submenu = shellMenu
        main.addItem(shell)

        let edit = NSMenuItem()
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Copy", action: #selector(MetalGridView.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Paste", action: #selector(MetalGridView.paste(_:)), keyEquivalent: "v")
        editMenu.addItem(.separator())
        editMenu.addItem(withTitle: "Find…", action: #selector(MetalGridView.findAction(_:)), keyEquivalent: "")
        editMenu.addItem(withTitle: "Find Next", action: #selector(MetalGridView.findNextAction(_:)), keyEquivalent: "")
        editMenu.addItem(withTitle: "Find Previous", action: #selector(MetalGridView.findPreviousAction(_:)), keyEquivalent: "")
        edit.submenu = editMenu
        main.addItem(edit)

        // Chords are not shown here: the keymap owns them and is remappable
        // (`vterm keys`, Settings → Keys).
        let blocks = NSMenuItem()
        let blocksMenu = NSMenu(title: "Blocks")
        blocksMenu.addItem(withTitle: "Previous Prompt", action: #selector(MetalGridView.previousPrompt(_:)), keyEquivalent: "")
        blocksMenu.addItem(withTitle: "Next Prompt", action: #selector(MetalGridView.nextPrompt(_:)), keyEquivalent: "")
        blocksMenu.addItem(.separator())
        blocksMenu.addItem(
            withTitle: "Select Previous Block", action: #selector(MetalGridView.selectPreviousBlock(_:)), keyEquivalent: ""
        )
        blocksMenu.addItem(withTitle: "Select Next Block", action: #selector(MetalGridView.selectNextBlock(_:)), keyEquivalent: "")
        blocksMenu.addItem(.separator())
        blocksMenu.addItem(withTitle: "Copy Command", action: #selector(MetalGridView.copyCommand(_:)), keyEquivalent: "")
        blocksMenu.addItem(withTitle: "Copy Output", action: #selector(MetalGridView.copyOutput(_:)), keyEquivalent: "")
        blocksMenu.addItem(withTitle: "Copy Command and Output", action: #selector(MetalGridView.copyBoth(_:)), keyEquivalent: "")
        blocksMenu.addItem(withTitle: "Copy as HTML", action: #selector(MetalGridView.exportBlock(_:)), keyEquivalent: "")
        blocksMenu.addItem(.separator())
        blocksMenu.addItem(withTitle: "Re-input Command", action: #selector(MetalGridView.reinputCommand(_:)), keyEquivalent: "")
        blocksMenu.addItem(withTitle: "Re-input as Root", action: #selector(MetalGridView.reinputSudo(_:)), keyEquivalent: "")
        blocksMenu.addItem(withTitle: "Re-run Command", action: #selector(MetalGridView.rerunCommand(_:)), keyEquivalent: "")
        blocksMenu.addItem(.separator())
        blocksMenu.addItem(withTitle: "Toggle Bookmark", action: #selector(MetalGridView.toggleBookmark(_:)), keyEquivalent: "")
        blocksMenu.addItem(withTitle: "Clear Scrollback", action: #selector(MetalGridView.clearScrollback(_:)), keyEquivalent: "")
        blocksMenu.addItem(
            withTitle: "Toggle Sticky Command Header", action: #selector(MetalGridView.toggleStickyHeader(_:)), keyEquivalent: ""
        )
        blocks.submenu = blocksMenu
        main.addItem(blocks)

        let window = NSMenuItem()
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(withTitle: "Minimize", action: #selector(NSWindow.miniaturize(_:)), keyEquivalent: "m")
        window.submenu = windowMenu
        main.addItem(window)
        NSApp.mainMenu = main
        NSApp.windowsMenu = windowMenu
    }

    private func fatal(_ message: String) -> Never {
        FileHandle.standardError.write(Data("vambiant-term: \(message)\n".utf8))
        let alert = NSAlert()
        alert.messageText = "Vambiant Term cannot start"
        alert.informativeText = message
        alert.runModal()
        exit(2)
    }
}
