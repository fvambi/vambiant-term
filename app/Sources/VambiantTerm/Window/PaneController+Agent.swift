// Agent Mode (ADR-0011 D2): ⌘↩ or ⌘K sends the editor's text to the
// daemon's `ai.ask` with this session as context and shows the answer in
// the pane's conversation panel as it streams (`ai.chunk`, `ai.done`,
// `ai.error` notifications). One request per pane at a time.

import AppKit

extension PaneController {
    var agentPanel: AgentPanel {
        container.agent
    }

    func wireAgent() {
        container.input.editor.onAgent = { [weak self] text in self?.askFromEditor(text) }
        container.input.editor.onEscape = { [weak self] in self?.hideAgent() }
        agentPanel.onClose = { [weak self] in self?.hideAgent() }
        agentPanel.onStage = { [weak self] command in self?.stage(command) }
    }

    /// The editor's text as a question; empty text just opens the panel.
    func askFromEditor(_ text: String? = nil) {
        let prompt = (text ?? container.input.editor.string).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty else {
            showAgent()
            return
        }
        container.input.editor.string = ""
        container.input.editor.didChangeText()
        ask(prompt, feature: "ask")
    }

    /// `ai.explain_last_failure`: the daemon's context already carries the
    /// last blocks; the question names the one that failed.
    func explainLastFailure() {
        guard let failed = blocks.commands.last(where: { $0.failed && $0.cmdline != nil }), let cmd = failed.cmdline else {
            agentPanel.conversation.fail("no failed command in this session yet")
            showAgent()
            return
        }
        ask("Explain why `\(cmd)` failed (exit \(failed.exit ?? -1)) and how to fix it.", feature: "explain")
    }

    /// The thinking row names the route (`ask`, `explain`), not a profile:
    /// the daemon resolves the profile and the answer reports the real one.
    func ask(_ prompt: String, feature: String) {
        guard agentPanel.conversation.ask(prompt, profile: "route \(feature)") else {
            NSSound.beep()
            return
        }
        showAgent()
        let params = AgentAskParams(
            prompt: prompt, feature: feature, session: session?.id, history: agentPanel.conversation.history
        )
        let daemon = self.daemon
        // The same shape as `runFind`: the blocking call off the main
        // thread, the hop back explicit, `self` never sent across.
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let result: Result<AgentRequestReply, Error> = Result { try daemon.call("ai.ask", params: params) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self else { return }
                    switch result {
                    case let .success(reply): self.agentPanel.conversation.request = reply.request
                    case let .failure(error): self.agentPanel.conversation.fail(Self.agentMessage(error))
                    }
                }
            }
        }
    }

    /// `ai.chunk` / `ai.done` / `ai.error` for this pane's session; only
    /// the request in flight is applied.
    func handleAgent(event method: String, params: JSONValue) {
        guard let request = params[path: "request"]?.stringValue, request == agentPanel.conversation.request else {
            return
        }
        switch method {
        case "ai.chunk":
            agentPanel.conversation.append(delta: params[path: "delta"]?.stringValue ?? "")
        case "ai.done":
            let seconds = agentPanel.conversation.since.map { Date().timeIntervalSince($0) } ?? 0
            if let data = try? JSONEncoder().encode(params),
               let reply = try? JSONDecoder().decode(AgentAskReply.self, from: data) {
                agentPanel.conversation.answer(reply.answer(seconds: seconds))
            } else {
                agentPanel.conversation.fail("unreadable ai.done from the daemon")
            }
        case "ai.error":
            agentPanel.conversation.fail(params[path: "message"]?.stringValue ?? "request failed")
        default:
            break
        }
    }

    private static func agentMessage(_ error: Error) -> String {
        let text = "\(error)"
        return text.hasPrefix("ai.ask: ") ? String(text.dropFirst(8)) : text
    }

    func showAgent() {
        guard agentPanel.isHidden else { return }
        agentPanel.isHidden = false
        container.needsLayout = true
        container.input.setHint("ESC for terminal  ·  ⌘↩ ask  ·  ⇧↩ newline")
    }

    func hideAgent() {
        guard !agentPanel.isHidden else { return }
        agentPanel.isHidden = true
        container.needsLayout = true
        container.input.setHint("⌘↩ for new agent  ·  ⇧↩ newline")
        focus()
    }

    /// Into the editor, not the shell: the user reads it and presses ↩.
    private func stage(_ command: String) {
        container.input.editor.string = command
        container.input.editor.didChangeText()
        container.window?.makeFirstResponder(container.input.editor)
    }
}
