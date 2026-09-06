// The command palette's sources and what a pick does (docs/01 C1.10,
// Warp's ⌘P): actions with their chords, sessions, history, repo files,
// workflows. Picks stage text into the editor or focus a pane; nothing
// here runs a command.

import AppKit

extension AppDelegate {
    /// ⌘⇧P: actions with their chords, every session, the daemon's
    /// history and the repo's files (Warp's `actions:` `sessions:`
    /// `history:` `files:` scopes). Files arrive asynchronously.
    func showPalette(for controller: TerminalWindowController, pane: PaneController, query: String = "") {
        var items: [PaletteItem] = []
        let bindings = configModel.snapshot?.keymap.bindings ?? []
        for a in configModel.snapshot?.actions ?? [] {
            // The macOS chord when both profiles bind the action.
            let bound = bindings.filter { $0.action == a.id }
            let chord = (bound.first { !$0.chord.hasPrefix("prefix ") } ?? bound.first)?.chord ?? ""
            items.append(PaletteItem(kind: .action(id: a.id), title: a.label, detail: chord))
        }
        for w in windows {
            for p in w.container.panes {
                items.append(PaletteItem(
                    kind: .session(paneID: ObjectIdentifier(p)), title: p.blocks.commands.last?.cmdline ?? p.title,
                    detail: p.cwd.map(GitProbe.abbreviated) ?? ""
                ))
            }
        }
        for h in pane.history(prefix: "") {
            items.append(PaletteItem(kind: .history(command: h), title: h, detail: "history"))
        }
        struct CwdParams: Encodable {
            let cwd: String?
        }
        if let reply: WorkflowsReply = try? daemon.call("workflows.list", params: CwdParams(cwd: pane.cwd)) {
            for w in reply.workflows {
                let args = w.arguments.map { "{{\($0.name)}}" }.joined(separator: " ")
                items.append(PaletteItem(
                    kind: .workflow(w), title: w.name,
                    detail: [w.description, args, w.warp ? "warp" : "workflow"].filter { !$0.isEmpty }.joined(separator: " · ")
                ))
            }
            for p in reply.problems {
                NSLog("workflow skipped: %@", p)
            }
        }
        palette.onPick = { [weak self] item in self?.perform(item, controller: controller, pane: pane) }
        palette.present(items: items, theme: renderer.theme, over: controller.window, query: query)
        if let cwd = pane.cwd {
            let snapshot = items
            GitProbe.files(in: cwd) { [weak self] files in
                DispatchQueue.main.async {
                    MainActor.assumeIsolated {
                        guard let self, self.palette.isVisible else { return }
                        self.palette.update(items: snapshot + files.map {
                            PaletteItem(kind: .file(path: $0), title: $0, detail: "file")
                        })
                    }
                }
            }
        }
    }

    func perform(_ item: PaletteItem, controller: TerminalWindowController, pane: PaneController) {
        switch item.kind {
        case let .action(id):
            let meta = configModel.snapshot?.actions.first { $0.id == id }
            controller.perform(ShellAction.from(id: id, label: meta?.label ?? id, milestone: meta?.milestone ?? "?"), on: pane)
        case let .session(paneID):
            for w in windows {
                if let p = w.container.panes.first(where: { ObjectIdentifier($0) == paneID }) {
                    w.window?.makeKeyAndOrderFront(nil)
                    w.container.focus(p)
                }
            }
        case let .history(command):
            pane.container.input.editor.string = command
            pane.container.input.editor.didChangeText()
            pane.focus()
        case let .file(path):
            pane.container.input.editor.insertText(path, replacementRange: pane.container.input.editor.selectedRange())
            pane.focus()
        case let .workflow(workflow):
            let stage: (String) -> Void = { [weak pane] command in
                guard let pane else { return }
                pane.container.input.editor.string = command
                pane.container.input.editor.didChangeText()
                pane.focus()
            }
            if workflow.arguments.isEmpty {
                stage(workflow.render([:]))
            } else if let window = controller.window {
                workflowSheet.onStage = stage
                workflowSheet.show(workflow, over: window)
            }
        }
    }
}
