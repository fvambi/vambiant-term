// Where events become notifications (docs/06 §8): finished long commands,
// agents that stopped or crashed, approvals arriving, Agent Mode ending.
// Each note lands in the mailbox; the policy sends it to a toast when
// its pane is off screen and to Notification Center when the app is not
// frontmost. The daemon posts approvals to Notification Center itself.

import AppKit

extension AppDelegate {
    var notificationPolicy: NotificationPolicy {
        guard let n = shellConfig?.notifications else { return NotificationPolicy() }
        return NotificationPolicy(
            awaitingInput: n.awaitingInput, agentFinished: NotificationPolicy.AgentFinished(rawValue: n.agentFinished) ?? .whenUnfocused,
            agentCrashed: n.agentCrashed, longCommandMs: n.longCommandMs, coalesceMs: n.coalesceWindowMs
        )
    }

    /// Notes from a broadcast, before the panes see it (so state changes
    /// can be compared with what the pane still holds).
    func notify(for method: String, params: JSONValue) {
        guard let session = params[path: "id"]?.stringValue else { return }
        switch method {
        case "session.block":
            if let block = Block.parse(item: params),
               let note = Note.longCommand(block, session: session, thresholdMs: notificationPolicy.longCommandMs) {
                deliver(note, source: .longCommand)
            }
        case "session.event" where params[path: "event.kind"]?.stringValue == "password_prompt":
            let line = params[path: "event.line"]?.stringValue ?? "password prompt"
            deliver(
                Note(
                    id: "pw-\(session)-\(Int(Date().timeIntervalSince1970))",
                    kind: .request,
                    title: "Waiting for a password",
                    body: line,
                    session: session,
                    at: Date()
                ),
                source: .passwordPrompt
            )
        case "session.changed":
            let state = params[path: "state"]?.stringValue
            let pane = pane(for: session)
            guard let pane, pane.agentKind != nil, pane.agentKind != "generic", pane.sessionState != state else { return }
            if state == "stopped" {
                deliver(
                    Note(id: "stop-\(session)", kind: .complete, title: "Agent finished", body: pane.title, session: session, at: Date()),
                    source: .agentFinished
                )
            } else if state == "crashed" {
                deliver(
                    Note(id: "crash-\(session)", kind: .error, title: "Agent crashed", body: pane.title, session: session, at: Date()),
                    source: .agentCrashed
                )
            }
        case "ai.done":
            let text = params[path: "text"]?.stringValue ?? ""
            let first = text.split(separator: "\n").first.map(String.init) ?? "done"
            deliver(
                Note(
                    id: "ai-\(params[path: "request"]?.stringValue ?? session)",
                    kind: .complete,
                    title: "Agent answered",
                    body: first,
                    session: session,
                    at: Date()
                ),
                source: .agentMode
            )
            // The budget banner (docs/06 §8): once per threshold crossing.
            if let data = try? JSONEncoder().encode(params[path: "budget"] ?? .null),
               let budget = try? JSONDecoder().decode(AgentBudget.self, from: data),
               let warning = budget.warning, warning != lastBudgetWarning {
                lastBudgetWarning = warning
                let reached = warning.hasPrefix("AI budget reached")
                deliver(
                    Note(
                        id: "budget-\(Int(Date().timeIntervalSince1970))", kind: reached ? .error : .info,
                        title: reached ? "AI budget reached" : "AI budget warning", body: warning, session: nil, at: Date()
                    ),
                    source: .budget
                )
            }
        case "ai.error":
            let message = params[path: "message"]?.stringValue ?? "request failed"
            deliver(
                Note(
                    id: "ai-\(params[path: "request"]?.stringValue ?? session)",
                    kind: .error,
                    title: "Agent request failed",
                    body: message,
                    session: session,
                    at: Date()
                ),
                source: .agentMode
            )
        default:
            break
        }
    }

    /// New approvals since the last inbox: one note each, coalesced.
    func notify(newApprovals items: [InboxItem]) {
        for item in items {
            deliver(
                Note(
                    id: "approval-\(item.id)",
                    kind: .request,
                    title: "\(item.sessionName) asks to run \(item.request.tool)",
                    body: item.summary,
                    session: item.session,
                    at: Date()
                ),
                source: .approval
            )
        }
    }

    func deliver(_ note: Note, source: NoteSource) {
        mailbox.coalesceWindow = Double(notificationPolicy.coalesceMs) / 1000
        let stored = mailbox.add(note)
        let pane = note.session.flatMap(pane(for:))
        let visible = pane.map { p in windows.contains { $0.window?.isKeyWindow == true && $0.container.focused === p } } ?? false
        let route = notificationPolicy.route(note.kind, source: source, appActive: NSApp.isActive, paneVisible: visible)
        if route.toast, let controller = windows.first(where: { $0.window?.isKeyWindow == true }) ?? windows.first {
            controller.toasts.show(stored, theme: renderer.theme)
        }
        if route.desktop {
            notifier.post(stored)
        }
        mailboxSheet.update(mailbox)
    }

    func pane(for session: String) -> PaneController? {
        windows.flatMap(\.container.panes).first { $0.session?.id == session }
    }

    /// Focus the note's session, marking the note read.
    func open(note: Note) {
        mailbox.markRead(id: note.id)
        mailboxSheet.update(mailbox)
        guard let session = note.session, let pane = pane(for: session),
              let controller = windows.first(where: { $0.container.panes.contains { $0 === pane } }) else { return }
        controller.window?.makeKeyAndOrderFront(nil)
        controller.container.focus(pane)
    }

    /// The mailbox sheet over `window`.
    func showMailbox(for window: NSWindow) {
        guard mailboxSheet.window.sheetParent == nil else { return }
        mailboxSheet.onOpen = { [weak self] note in self?.open(note: note) }
        mailboxSheet.onMarkAllRead = { [weak self] in
            guard let self else { return }
            mailbox.markAllRead()
            mailboxSheet.update(mailbox)
        }
        mailboxSheet.update(mailbox)
        window.beginSheet(mailboxSheet.window) { [weak self] _ in
            guard let self else { return }
            mailbox.markAllRead()
        }
    }
}
