// `VAMBIANT_TERM_SCREENSHOT=<png>`: the diagnostic that drives one pane
// through every visible feature and writes frames, for eyes on a build
// that CI cannot look at.

import AppKit
import CVambiantTerm

extension AppDelegate {
    /// AppKit views through the display cache; the Metal grid inside may
    /// come out blank, which is why the grid has its own capture.
    static func captureAppKit(_ view: NSView?, to path: String) {
        guard let view, let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
        view.cacheDisplay(in: view.bounds, to: rep)
        if let png = rep.representation(using: .png, properties: [:]) {
            try? png.write(to: URL(fileURLWithPath: path))
        }
    }

    /// `VAMBIANT_TERM_SCREENSHOT=<png>`: drive one pane through attributes,
    /// a failing command, block selection, a bookmark, a find and a scroll,
    /// then write the frame and log the overlay state.
    func runScreenshotDiagnostic(pane: PaneController, shot: String) {
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
                // The sidebar, so the window shot shows Warp's vertical tabs.
                (self.windows.first)?.toggleSidebar(nil)
                // The palette, captured from its own panel.
                if let controller = self.windows.first {
                    self.showPalette(for: controller, pane: pane)
                    self.palette.contentView?.layoutSubtreeIfNeeded()
                    Self.captureAppKit(self.palette.contentView, to: shot + ".palette.png")
                    self.palette.orderOut(nil)
                }
                // A text selection across two rows: the highlight is in the
                // grid capture, the copied text in the log.
                let selTop = pane.view.viewportTop
                pane.view.textSelection = TextSelection(
                    anchor: GridPoint(row: selTop + 1, col: 2),
                    head: GridPoint(row: selTop + 2, col: 18)
                )
                NSLog("screenshot: selection text: %@", pane.view.selectedText()?.replacingOccurrences(of: "\n", with: "⏎") ?? "nil")
                pane.view.captureNext(to: shot)
                // The whole window (chrome, chips, editor) through AppKit's
                // display cache; the Metal grid inside may come out blank.
                Self.captureAppKit(pane.container.window?.contentView, to: shot + ".window.png")
                // Agent Mode: a real `ai.ask` through the daemon. Without a
                // configured key the panel shows the daemon's refusal, which
                // is the honest state and enough to verify the layout.
                pane.askFromEditor("what does grep -c do?")
            }
        }
        scheduleAgentCaptures(pane: pane, shot: shot)
    }

    /// The Agent Mode panel mid-answer, then the safety confirm sheet.
    private func scheduleAgentCaptures(pane: PaneController, shot: String) {
        Timer.scheduledTimer(withTimeInterval: 8.5, repeats: false) { _ in
            MainActor.assumeIsolated {
                pane.showAgent()
                let tools = pane.agentPanel.conversation.turns.compactMap { turn -> String? in
                    if case let .tool(call) = turn {
                        return "\(call.command) → \(call.statusLine)"
                    }
                    return nil
                }
                NSLog(
                    "screenshot: agent done: %@; tools: %@",
                    AgentPanel.title(for: pane.agentPanel.conversation),
                    tools.joined(separator: "; ")
                )
                Self.captureAppKit(pane.container.window?.contentView, to: shot + ".agent-done.png")
            }
        }
        Timer.scheduledTimer(withTimeInterval: 5.5, repeats: false) { _ in
            MainActor.assumeIsolated {
                NSLog(
                    "screenshot: agent panel hidden %d, %@",
                    pane.agentPanel.isHidden ? 1 : 0,
                    AgentPanel.title(for: pane.agentPanel.conversation)
                )
                Self.captureAppKit(pane.container.window?.contentView, to: shot + ".agent.png")
                // Allow the agent's command through the card, as a user would.
                if let item = pane.approvals.first {
                    pane.decide(item, .allow(updatedCommand: nil))
                }
                // The safety confirm: a destructive line from the editor.
                pane.hideAgent()
                pane.submit("rm -rf /")
            }
        }
        Timer.scheduledTimer(withTimeInterval: 5.9, repeats: false) { _ in
            MainActor.assumeIsolated {
                // NSAlert's labels do not survive the display cache; log them.
                let sheet = pane.container.window?.attachedSheet
                let labels = (sheet?.contentView.map(Self.labels) ?? []).joined(separator: " | ")
                NSLog("screenshot: confirm sheet %@: %@", sheet == nil ? "absent" : "shown", labels)
                Self.captureAppKit(sheet?.contentView, to: shot + ".confirm.png")
                if let sheet {
                    pane.container.window?.endSheet(sheet, returnCode: .cancel)
                }
                if let script = ProcessInfo.processInfo.environment["VAMBIANT_TERM_SCREENSHOT_AGENT"] {
                    self.scheduleInboxCaptures(script: script, shot: shot)
                }
            }
        }
    }

    /// `VAMBIANT_TERM_SCREENSHOT_AGENT=<script>`: a fake agent that posts a
    /// permission request; the approval card and the inbox sheet follow.
    private func scheduleInboxCaptures(script: String, shot: String) {
        guard let controller = windows.first else { return }
        let agentPane = controller.openAgentPane(argv: ["/bin/sh", script], agent: "claude")
        NSLog("screenshot: fake agent pane %@", agentPane?.session?.id ?? agentPane?.lastError ?? "not created")
        Timer.scheduledTimer(withTimeInterval: 2.5, repeats: false) { _ in
            MainActor.assumeIsolated {
                let cards = controller.container.panes.compactMap { p in p.container.approval.isHidden ? nil : p }
                let labels = cards.map { Self.labels(in: $0.container.approval).joined(separator: " | ") }
                NSLog("screenshot: inbox %d pending, cards %d: %@", self.inbox.count, cards.count, labels.joined(separator: " || "))
                Self.captureAppKit(controller.window?.contentView, to: shot + ".inbox-card.png")
                if let window = controller.window {
                    self.showInbox(for: window)
                }
            }
        }
        Timer.scheduledTimer(withTimeInterval: 3.2, repeats: false) { _ in
            MainActor.assumeIsolated {
                let content = self.inboxSheet.window.contentView
                NSLog("screenshot: inbox sheet: %@", (content.map(Self.labels) ?? []).joined(separator: " | "))
                Self.captureAppKit(content, to: shot + ".inbox.png")
            }
        }
    }

    /// Every text field's text under `view`, in order.
    static func labels(in view: NSView) -> [String] {
        var out: [String] = []
        if let field = view as? NSTextField, !field.stringValue.isEmpty {
            out.append(field.stringValue)
        }
        for sub in view.subviews {
            out += labels(in: sub)
        }
        return out
    }
}
