// The sidebar's rows (docs/06 §2, Warp's vertical tabs): repositories,
// then the sessions inside each, with Warp's metadata per row. Pure, so
// grouping and labels are tested without a window.

import Foundation

struct SidebarSession: Equatable, Sendable, Identifiable {
    /// Pane identity (stable across refreshes).
    let id: ObjectIdentifier
    let name: String
    let cwd: String?
    let branch: String?
    let lastCommand: String?
    /// Daemon state (`idle`, `thinking`, `tool_running`, `awaiting_input`, …).
    let state: String?
    /// `claude`, `codex`, `generic`; nil until the daemon says.
    let agent: String?
    let diff: (added: Int, removed: Int)?
    let focused: Bool

    static func == (a: SidebarSession, b: SidebarSession) -> Bool {
        a.id == b.id && a.name == b.name && a.cwd == b.cwd && a.branch == b.branch && a.lastCommand == b.lastCommand
            && a.state == b.state && a.agent == b.agent && a.diff?.added == b.diff?.added
            && a.diff?.removed == b.diff?.removed && a.focused == b.focused
    }

    /// Warp's primary line: the last command, else the cwd, else the name.
    var title: String {
        if let lastCommand, !lastCommand.isEmpty {
            return lastCommand
        }
        if let cwd {
            return GitProbe.abbreviated(cwd)
        }
        return name
    }

    /// docs/06 §2 state glyphs: shapes, never colour alone.
    var stateGlyph: String {
        switch state {
        case "thinking": "◐"
        case "tool_running": "⚙"
        case "awaiting_input": "✋"
        case "crashed": "✕"
        case "stopped": "■"
        case "starting": "◌"
        default: "○"
        }
    }

    var stateLabel: String {
        switch state {
        case "thinking": "thinking"
        case "tool_running": "tool running"
        case "awaiting_input": "waiting for you"
        case "crashed": "crashed"
        case "stopped": "stopped"
        case "starting": "starting"
        default: "idle"
        }
    }

    var diffText: String? {
        guard let diff, diff.added + diff.removed > 0 else { return nil }
        return "+\(diff.added) -\(diff.removed)"
    }
}

enum SidebarRow: Equatable, Sendable {
    /// A repository (or "scratch" for sessions outside any repo).
    case repo(name: String, path: String?)
    case session(SidebarSession)
}

enum SidebarModel {
    /// Rows grouped by repository, repos in first-seen order, sessions in
    /// the given order; sessions outside a repo go last under "scratch".
    static func rows(for sessions: [SidebarSession], repoRoot: (String) -> String?) -> [SidebarRow] {
        var groups: [(path: String?, name: String, sessions: [SidebarSession])] = []
        for s in sessions {
            let root = s.cwd.flatMap(repoRoot)
            if let i = groups.firstIndex(where: { $0.path == root }) {
                groups[i].sessions.append(s)
            } else {
                let name = root.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "scratch"
                groups.append((root, name, [s]))
            }
        }
        // Scratch last, everything else in first-seen order.
        let ordered = groups.filter { $0.path != nil } + groups.filter { $0.path == nil }
        var rows: [SidebarRow] = []
        for g in ordered {
            rows.append(.repo(name: g.name, path: g.path))
            rows.append(contentsOf: g.sessions.map(SidebarRow.session))
        }
        return rows
    }

    /// Rows whose session matches `query` (title, cwd, branch, last command).
    static func filter(_ rows: [SidebarRow], query: String) -> [SidebarRow] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return rows }
        var out: [SidebarRow] = []
        var pendingRepo: SidebarRow?
        for row in rows {
            switch row {
            case .repo:
                pendingRepo = row
            case let .session(s):
                let hay = [s.title, s.cwd ?? "", s.branch ?? "", s.name].joined(separator: " ").lowercased()
                if hay.contains(q) {
                    if let r = pendingRepo {
                        out.append(r)
                        pendingRepo = nil
                    }
                    out.append(row)
                }
            }
        }
        return out
    }
}
