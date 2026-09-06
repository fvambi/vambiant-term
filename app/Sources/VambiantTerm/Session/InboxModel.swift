// The approval inbox as the daemon reports it (`inbox.list`,
// `inbox.changed`): every pending request with its verdict. Pure, so the
// summaries the card and the sheet show are tested without a window.

import Foundation

struct InboxRequest: Decodable, Equatable, Sendable {
    let id: String
    let tool: String
    let input: JSONValue
    var reason: String?
    let source: String
}

struct InboxItem: Decodable, Equatable, Sendable, Identifiable {
    let id: String
    let session: String
    let sessionName: String
    let request: InboxRequest
    let hookEvent: String
    let requestedAt: String
    let waitingSecs: Int
    let promptShown: Bool
    var verdict: SafetyVerdictBody?
    var floor: SafetyFloor?

    enum CodingKeys: String, CodingKey {
        case id, session, request, verdict, floor
        case sessionName = "session_name"
        case hookEvent = "hook_event"
        case requestedAt = "requested_at"
        case waitingSecs = "waiting_secs"
        case promptShown = "prompt_shown"
    }

    /// The command line for shell tools, nil otherwise.
    var command: String? {
        request.input[path: "command"]?.stringValue
    }

    /// The file for edit/write tools.
    var filePath: String? {
        request.input[path: "file_path"]?.stringValue ?? request.input[path: "path"]?.stringValue
    }

    /// `Bash` → the command; `Edit src/x.ts (+12 −4)`; else the tool and its input.
    var summary: String {
        if let command {
            return command
        }
        if let filePath {
            var s = "\(request.tool) \(filePath)"
            if let old = request.input[path: "old_string"]?.stringValue, let new = request.input[path: "new_string"]?.stringValue {
                s += " (+\(new.split(separator: "\n").count) −\(old.split(separator: "\n").count))"
            } else if let content = request.input[path: "content"]?.stringValue {
                s += " (+\(content.split(separator: "\n").count))"
            }
            return s
        }
        let data = (try? JSONEncoder().encode(request.input)).flatMap { String(data: $0, encoding: .utf8) } ?? ""
        return "\(request.tool) \(data.prefix(120))"
    }

    /// `⚠ destructive — rm-recursive: \`/\` — deletes … (outside the worktree)`;
    /// nil when there is no verdict, "benign" when there is nothing to say.
    var verdictLine: String? {
        guard let verdict else { return nil }
        guard let first = verdict.findings.first else {
            return verdict.parseError.map { "⚠ unparseable — \($0)" } ?? "✓ benign"
        }
        var line = "⚠ \(verdict.class) — \(first.rule): `\(first.token)` — \(first.detail)"
        if first.outsideWorktree, first.class == "destructive" {
            line += " (outside the worktree)"
        }
        if first.protectedBranch {
            line += " (protected branch)"
        }
        return line
    }

    /// The floor reason, spelled out, when the request can never be auto-approved.
    var floorLine: String? {
        guard let floor else { return nil }
        switch floor.reason {
        case "destructive_outside_worktree": return "never auto-approved: destructive outside the worktree"
        case "credential_read": return "never auto-approved: reads credentials"
        case "obfuscated": return "never auto-approved: obfuscated"
        case "force_push_protected": return "never auto-approved: force-push to a protected branch"
        case "generic_adapter": return "never auto-approved: the session is observed heuristically"
        case "parse_failed": return "never auto-approved: the command line could not be parsed"
        default: return "never auto-approved: \(floor.reason)"
        }
    }

    /// Why it was asked (docs/06 §3). Rule evaluation on this path is M8;
    /// until then the honest answer is that autonomy is off.
    var askedBecause: String {
        promptShown
            ? "the agent shows its own prompt now — answer there"
            : "autonomy is off: every \(request.tool) request is asked"
    }

    var waitingLabel: String {
        waitingSecs < 60 ? "\(waitingSecs)s" : "\(waitingSecs / 60)m \(waitingSecs % 60)s"
    }
}

/// The decision as `inbox.decide` expects it.
enum InboxDecision: Encodable, Equatable, Sendable {
    case allow(updatedCommand: String?)
    case deny(reason: String)

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case let .allow(updated):
            try c.encode("allow", forKey: .behavior)
            if let updated {
                try c.encode(["command": updated], forKey: .updatedInput)
            } else {
                try c.encodeNil(forKey: .updatedInput)
            }
        case let .deny(reason):
            try c.encode("deny", forKey: .behavior)
            try c.encode(reason, forKey: .reason)
        }
    }

    enum Keys: String, CodingKey {
        case behavior, reason
        case updatedInput = "updated_input"
    }
}

struct InboxDecideParams: Encodable, Sendable {
    let id: String
    let decision: InboxDecision
}

struct InboxList: Equatable, Sendable {
    private(set) var items: [InboxItem] = []

    /// Oldest first: the one that has waited longest is answered first.
    mutating func replace(with new: [InboxItem]) {
        items = new.sorted { $0.requestedAt < $1.requestedAt }
    }

    func pending(for session: String) -> [InboxItem] {
        items.filter { $0.session == session }
    }

    var count: Int {
        items.count
    }
}
