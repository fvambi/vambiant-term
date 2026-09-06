// The daemon's `policy.classify` reply (docs/05 §5): the class, the rules
// that fired with their tokens, the floor reason when the command can
// never be auto-approved, and what `[safety]` says to do when a human
// runs it. Pure data; the confirm sheet and the labels are built from it.

import Foundation

struct SafetyFinding: Decodable, Equatable, Sendable {
    let `class`: String
    let rule: String
    let token: String
    let detail: String
    let outsideWorktree: Bool
    let protectedBranch: Bool
    let host: String?

    enum CodingKeys: String, CodingKey {
        case `class`, rule, token, detail, host
        case outsideWorktree = "outside_worktree"
        case protectedBranch = "protected_branch"
    }

    /// The daemon omits the flags when false.
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        `class` = try c.decode(String.self, forKey: .class)
        rule = try c.decode(String.self, forKey: .rule)
        token = try c.decode(String.self, forKey: .token)
        detail = try c.decode(String.self, forKey: .detail)
        outsideWorktree = try c.decodeIfPresent(Bool.self, forKey: .outsideWorktree) ?? false
        protectedBranch = try c.decodeIfPresent(Bool.self, forKey: .protectedBranch) ?? false
        host = try c.decodeIfPresent(String.self, forKey: .host)
    }
}

struct SafetyVerdictBody: Decodable, Equatable, Sendable {
    let `class`: String
    let findings: [SafetyFinding]
    var parseError: String?
    let commands: Int

    enum CodingKeys: String, CodingKey {
        case `class`, findings, commands
        case parseError = "parse_error"
    }
}

struct SafetyFloor: Decodable, Equatable, Sendable {
    let reason: String
    var token: String?
    var detail: String?
}

struct SafetyVerdict: Decodable, Equatable, Sendable {
    let verdict: SafetyVerdictBody
    var floor: SafetyFloor?
    /// `allow`, `warn`, `confirm` or `block`.
    let decision: String

    var isBenign: Bool {
        verdict.class == "benign"
    }

    var needsConfirm: Bool {
        decision == "confirm"
    }

    var isBlocked: Bool {
        decision == "block"
    }

    /// `destructive` or `destructive · never auto`.
    var label: String {
        floor == nil ? verdict.class : "\(verdict.class) · never auto"
    }

    /// One line per finding: `rm-recursive: \`/\` — deletes … (outside the worktree)`.
    var explanation: String {
        var lines = verdict.findings.map { f in
            var line = "\(f.rule): `\(f.token)` — \(f.detail)"
            if f.outsideWorktree, f.class == "destructive" {
                line += " (outside the worktree)"
            }
            if f.protectedBranch {
                line += " (protected branch)"
            }
            return line
        }
        if let error = verdict.parseError {
            lines.append("could not be parsed: \(error)")
        }
        return lines.joined(separator: "\n")
    }
}
