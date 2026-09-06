// Notifications as data (docs/06 §8, 12 §G3): what happened, in which
// session, whether it was seen, coalesced per session and kind within
// `[notifications] coalesce_window_ms`. The policy decides where a note
// goes — toast, Notification Center, or nowhere — from the config and
// whether the pane is on screen. Pure, so both are tested without a window.

import Foundation

enum NoteKind: String, Equatable, Sendable {
    case complete, request, error, info
}

struct Note: Equatable, Sendable, Identifiable {
    let id: String
    let kind: NoteKind
    var title: String
    var body: String
    let session: String?
    let at: Date
    var read = false
    /// How many events this note stands for after coalescing.
    var count = 1
}

struct Mailbox: Equatable, Sendable {
    enum Filter: Equatable, Sendable {
        case all, unread, errors
    }

    private(set) var notes: [Note] = []
    var coalesceWindow: TimeInterval = 10
    static let cap = 200

    /// Adds a note, or folds it into the newest one of the same session
    /// and kind inside the coalescing window ("3 approvals waiting").
    /// Returns the note as stored.
    @discardableResult
    mutating func add(_ note: Note) -> Note {
        if let i = notes.firstIndex(where: { $0.session == note.session && $0.kind == note.kind && !$0.read }),
           note.at.timeIntervalSince(notes[i].at) < coalesceWindow {
            var merged = notes[i]
            merged.count += 1
            merged.body = note.body
            merged.title = Self.coalescedTitle(note.title, count: merged.count, kind: note.kind)
            notes.remove(at: i)
            notes.insert(merged, at: 0)
            return merged
        }
        notes.insert(note, at: 0)
        if notes.count > Self.cap {
            notes.removeLast(notes.count - Self.cap)
        }
        return note
    }

    static func coalescedTitle(_ title: String, count: Int, kind: NoteKind) -> String {
        switch kind {
        case .request: "\(count) approvals waiting"
        case .complete: "\(count) commands finished"
        case .error: "\(count) failures"
        case .info: title
        }
    }

    var unread: Int {
        notes.filter { !$0.read }.count
    }

    func filtered(_ filter: Filter) -> [Note] {
        switch filter {
        case .all: notes
        case .unread: notes.filter { !$0.read }
        case .errors: notes.filter { $0.kind == .error }
        }
    }

    mutating func markRead(id: String) {
        if let i = notes.firstIndex(where: { $0.id == id }) {
            notes[i].read = true
        }
    }

    mutating func markAllRead() {
        for i in notes.indices {
            notes[i].read = true
        }
    }
}

/// `[notifications]` as the app applies it.
struct NotificationPolicy: Equatable, Sendable {
    enum AgentFinished: String, Equatable, Sendable {
        case always, whenUnfocused = "when_unfocused", never
    }

    var awaitingInput = true
    var agentFinished = AgentFinished.whenUnfocused
    var agentCrashed = true
    var longCommandMs = 30000
    var coalesceMs = 10000

    /// Where a note goes. `paneVisible` means its pane is the focused one
    /// of the key window; `appActive` that the app is frontmost.
    /// Approvals never go to Notification Center from here: the daemon
    /// posts those itself, so the CLI-only user gets them too.
    func route(_ kind: NoteKind, source: NoteSource, appActive: Bool, paneVisible: Bool) -> (toast: Bool, desktop: Bool) {
        let onScreen = appActive && paneVisible
        switch source {
        case .longCommand:
            return (toast: appActive && !paneVisible, desktop: !appActive)
        case .approval:
            return (toast: awaitingInput && appActive && !paneVisible, desktop: false)
        case .agentFinished:
            switch agentFinished {
            case .never: return (false, false)
            case .always: return (toast: appActive && !paneVisible, desktop: !appActive)
            case .whenUnfocused: return (toast: false, desktop: !appActive)
            }
        case .agentCrashed:
            return agentCrashed ? (toast: appActive, desktop: !appActive) : (false, false)
        case .agentMode:
            return (toast: appActive && !paneVisible, desktop: !appActive && !onScreen)
        case .budget:
            // An in-app banner (docs/06 §8): always a toast, never the desktop.
            return (toast: appActive, desktop: false)
        case .passwordPrompt:
            return (toast: appActive && !paneVisible, desktop: !appActive)
        }
    }
}

enum NoteSource: Equatable, Sendable {
    case longCommand, approval, agentFinished, agentCrashed, agentMode, budget, passwordPrompt
}

extension Note {
    /// A finished command block, when it ran at least `thresholdMs`.
    static func longCommand(_ block: Block, session: String, thresholdMs: Int, at: Date = Date()) -> Note? {
        guard block.isCommand, let ms = block.durationMs, ms >= UInt64(max(0, thresholdMs)) else { return nil }
        let exit = block.exit ?? -1
        let secs = Double(ms) / 1000
        let duration = secs >= 60 ? String(format: "%dm %02ds", Int(secs) / 60, Int(secs) % 60) : String(format: "%.1fs", secs)
        return Note(
            id: "block-\(session)-\(block.seq)", kind: exit == 0 ? .complete : .error,
            title: exit == 0 ? "Command finished · \(duration)" : "Command failed (exit \(exit)) · \(duration)",
            body: block.cmdline ?? "(command line unknown)", session: session, at: at
        )
    }
}
