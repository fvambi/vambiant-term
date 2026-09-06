// Which daemon sessions the app reattaches (docs/01 A3.6, 12 §C1/C9):
// running ones, oldest first, so a restart brings every tab back in the
// order it was opened; and, for ⌘⇧T, the newest running session no pane
// shows. Pure, so the rules are tested without a daemon.

import Foundation

enum SessionRestore {
    /// Sessions still alive in the daemon, oldest first.
    static func running(_ sessions: [SessionInfo]) -> [SessionInfo] {
        sessions
            .filter { $0.orphaned != true && $0.state != "stopped" && $0.state != "crashed" }
            .sorted { ($0.createdAt ?? "") < ($1.createdAt ?? "") }
    }

    /// The most recently created running session not in `attached`.
    static func reopenCandidate(_ sessions: [SessionInfo], attached: Set<String>) -> SessionInfo? {
        running(sessions).last { !attached.contains($0.id) }
    }
}
