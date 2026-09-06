// The approval card's decisions go straight to `inbox.decide`; the
// answer is the daemon's, shown verbatim when it refuses (a Codex edit,
// an expired hold). Opening the full inbox is the app's job.

import AppKit

extension PaneController {
    func wireInbox() {
        container.approval.onDecide = { [weak self] item, decision in self?.decide(item, decision) }
        container.approval.onOpenInbox = { [weak self] in
            guard let self, let window = container.window else { return }
            (NSApp.delegate as? AppDelegate)?.showInbox(for: window)
        }
    }

    /// `inbox.decide`; the daemon's refusal, if any, is shown in the hint.
    func decide(_ item: InboxItem, _ decision: InboxDecision) {
        do {
            try daemon.invoke("inbox.decide", params: InboxDecideParams(id: item.id, decision: decision))
        } catch {
            container.input.setHint("inbox: \("\(error)".replacingOccurrences(of: "inbox.decide: ", with: ""))")
            NSSound.beep()
        }
    }
}
