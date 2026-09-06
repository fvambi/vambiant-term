// The safety confirm on typed commands (docs/06 §5, docs/05 §5): the
// editor's line goes to the daemon's classifier first; `confirm` shows
// a sheet naming the rule and token, `block` refuses, `warn` runs with
// the verdict in the hint line. Classic mode is untouched: the shell's
// own editor never passes through here.

import AppKit

extension PaneController {
    /// `policy.classify` for a command line in this session's cwd; nil
    /// when the daemon cannot answer, which is shown, not hidden.
    func classify(_ command: String) -> SafetyVerdict? {
        struct Params: Encodable {
            let command: String
            let session: String?
        }
        return try? daemon.call("policy.classify", params: Params(command: command, session: session?.id))
    }

    /// Short label for a staged command, nil when benign or unknown.
    func safetyLabel(_ command: String) -> String? {
        guard let v = classify(command), !v.isBenign else { return nil }
        return v.label
    }

    /// The editor's line: classified, then confirmed, refused or sent.
    func submitChecked(_ text: String) {
        let line = text.replacingOccurrences(of: "\r\n", with: "\n")
        guard !line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            send(line)
            return
        }
        guard let verdict = classify(line) else {
            // Never lie: the classifier did not answer, so say so and
            // confirm as if it were unparseable.
            confirm(
                line,
                title: "The safety classifier did not answer",
                body: "vtermd's policy.classify failed; treat this as unclassified."
            )
            return
        }
        if verdict.isBlocked {
            let alert = NSAlert()
            alert.messageText = "Blocked by policy: \(verdict.verdict.class)"
            alert.informativeText = verdict.explanation
            alert.alertStyle = .critical
            alert.addButton(withTitle: "OK")
            present(alert) { _ in }
            container.input.editor.string = line
            container.input.editor.didChangeText()
            return
        }
        if verdict.needsConfirm {
            confirm(line, title: "Run \(verdict.verdict.class) command?", body: verdict.explanation)
            return
        }
        if verdict.decision == "warn" {
            container.input.setHint("⚠ \(verdict.verdict.class): \(verdict.explanation.split(separator: "\n").first ?? "")")
        }
        send(line)
    }

    private func confirm(_ line: String, title: String, body: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = "\(line)\n\n\(body)"
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Run")
        alert.addButton(withTitle: "Cancel")
        present(alert) { [weak self] response in
            guard let self else { return }
            if response == .alertFirstButtonReturn {
                self.send(line)
            } else {
                self.container.input.editor.string = line
                self.container.input.editor.didChangeText()
                self.focus()
            }
        }
    }

    private func present(_ alert: NSAlert, then: @escaping @MainActor (NSApplication.ModalResponse) -> Void) {
        if let window = container.window {
            alert.beginSheetModal(for: window) { response in
                MainActor.assumeIsolated { then(response) }
            }
        } else {
            then(alert.runModal())
        }
    }
}
