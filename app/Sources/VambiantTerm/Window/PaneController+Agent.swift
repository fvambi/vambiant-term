// Agent Mode (ADR-0011 D2): ⌘↩ or ⌘K sends the editor's text to the
// daemon's `ai.ask` with this session as context and shows the answer in
// the pane's conversation panel. The call blocks on the daemon, so it runs
// off the main thread; one request per pane at a time.

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
        let started = Date()
        Task.detached(priority: .userInitiated) { [weak self] in
            let result: Result<AgentAskReply, Error> = Result { try daemon.call("ai.ask", params: params) }
            let seconds = Date().timeIntervalSince(started)
            await MainActor.run {
                guard let self else { return }
                switch result {
                case let .success(reply): self.agentPanel.conversation.answer(reply.answer(seconds: seconds))
                case let .failure(error): self.agentPanel.conversation.fail(Self.agentMessage(error))
                }
                self.container.needsLayout = true
            }
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
