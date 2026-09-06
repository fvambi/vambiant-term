// Agent Mode's conversation (ADR-0011 D2, docs/06 §6): the turns one pane
// has exchanged with the daemon's `ai.ask`. Pure data so the staging rule
// and the footer are testable. Nothing here runs a command: proposals are
// staged into the editor and the user presses ↩ (CLAUDE.md rule 5).

import Foundation

struct AgentAnswer: Equatable, Sendable {
    var text: String
    var profile: String
    var model: String
    var inputTokens: Int
    var outputTokens: Int
    /// List-price estimate from the daemon, nil when the model is unpriced.
    var costUSD: Double?
    var redactions: Int
    /// The model hit `max_tokens`; the answer is cut off and says so.
    var truncated: Bool
    var seconds: Double
    /// Everything the model said in this run, when tool calls split it
    /// into segments; `text` is then only the last segment. History
    /// carries this, the panel shows the segments in place.
    var transcript: String?

    /// Commands the answer proposes: every line of a fenced `sh`/`bash`/
    /// `zsh`/`shell`/untagged block, and `$ `-prefixed lines in prose.
    var commands: [String] {
        Self.commands(in: text)
    }

    static func commands(in text: String) -> [String] {
        var out: [String] = []
        var inFence = false
        var fenceIsShell = false
        for raw in text.split(separator: "\n", omittingEmptySubsequences: false) {
            let line = String(raw)
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("```") {
                if inFence {
                    inFence = false
                } else {
                    inFence = true
                    let tag = trimmed.dropFirst(3).trimmingCharacters(in: .whitespaces).lowercased()
                    fenceIsShell = ["", "sh", "bash", "zsh", "shell", "console", "fish"].contains(tag)
                }
                continue
            }
            if inFence {
                if fenceIsShell, !trimmed.isEmpty, !trimmed.hasPrefix("#") {
                    out.append(trimmed.hasPrefix("$ ") ? String(trimmed.dropFirst(2)) : trimmed)
                }
            } else if trimmed.hasPrefix("$ "), trimmed.count > 2 {
                out.append(String(trimmed.dropFirst(2)))
            }
        }
        var seen = Set<String>()
        return out.filter { seen.insert($0).inserted }
    }

    /// `claude-sonnet-5 · 1.2k in / 340 out · ≈$0.0041 · 2 redactions · 3.2s`
    var footer: String {
        var parts = [model, "\(Self.count(inputTokens)) in / \(Self.count(outputTokens)) out"]
        if let costUSD {
            parts.append("≈$" + String(format: costUSD < 0.01 ? "%.4f" : "%.2f", costUSD))
        }
        if redactions > 0 {
            parts.append("\(redactions) redaction\(redactions == 1 ? "" : "s")")
        }
        parts.append(String(format: "%.1fs", seconds))
        if truncated {
            parts.append("cut off at max tokens")
        }
        return parts.joined(separator: " · ")
    }

    static func count(_ n: Int) -> String {
        n >= 1000 ? String(format: "%.1fk", Double(n) / 1000) : String(n)
    }
}

/// One `run_command` the model asked for (docs/06 §6, Warp's embedded
/// command block): it runs only after the inbox decides.
struct AgentToolCall: Equatable, Sendable {
    enum Status: Equatable, Sendable {
        case waiting, running
        case done(exit: Int)
        case denied(reason: String)
        case failed(message: String)
    }

    let id: String
    var command: String
    var why: String?
    var verdictClass: String?
    var floor: Bool
    /// `allow`/`ask`/`deny` from the policy engine, `applied` when a rule decided.
    var decision: String?
    var applied: Bool
    var status: Status = .waiting
    var output: String?

    var statusLine: String {
        switch status {
        case .waiting: applied ? "decided by policy" : "waiting for your approval — the card above decides"
        case .running: "running…"
        case let .done(exit): exit == 0 ? "exit 0" : "exit \(exit)"
        case let .denied(reason): "denied: \(reason)"
        case let .failed(message): "not run: \(message)"
        }
    }

    /// `destructive · never auto`, nil when benign or unknown.
    var label: String? {
        guard let verdictClass, verdictClass != "benign" else { return nil }
        return floor ? "\(verdictClass) · never auto" : verdictClass
    }
}

enum AgentTurn: Equatable, Sendable {
    case user(String)
    /// A finished text segment of a run that continued with a tool call.
    case text(String)
    /// A command the model asked to run.
    case tool(AgentToolCall)
    /// Warp's `Thought for N seconds ›` row, kept once the answer is in.
    case thought(seconds: Double)
    /// A request in flight; `since` drives the "Thinking for Ns" row.
    case thinking(profile: String, since: Date)
    /// Deltas arriving; `partial` is the text so far.
    case streaming(partial: String, since: Date)
    case answer(AgentAnswer)
    /// The daemon refused or the provider failed; the message is the
    /// daemon's, verbatim, so a redaction refusal reads as one.
    case failure(String)
}

struct AgentConversation: Equatable, Sendable {
    private(set) var turns: [AgentTurn] = []
    /// The daemon's id for the request in flight; events for any other
    /// request are stale and ignored.
    var request: String?

    /// A request is out (thinking or streaming).
    var isThinking: Bool {
        switch turns.last {
        case .thinking, .streaming: true
        default: false
        }
    }

    /// When the request in flight started.
    var since: Date? {
        switch turns.last {
        case let .thinking(_, since), let .streaming(_, since): since
        default: nil
        }
    }

    var isEmpty: Bool {
        turns.isEmpty
    }

    /// Appends the question and a thinking row. Returns false while a
    /// request is already in flight: one at a time per pane.
    @discardableResult
    mutating func ask(_ prompt: String, profile: String, at now: Date = Date()) -> Bool {
        guard !isThinking else { return false }
        turns.append(.user(prompt))
        turns.append(.thinking(profile: profile, since: now))
        return true
    }

    /// One delta of the answer; the thinking row becomes the live text.
    mutating func append(delta: String) {
        guard isThinking, let since else { return }
        var partial = ""
        if case let .streaming(text, _) = turns[turns.count - 1] {
            partial = text
        }
        turns[turns.count - 1] = .streaming(partial: partial + delta, since: since)
    }

    /// The model asked to run a command: the text streamed so far becomes
    /// a segment, the call goes in, and a thinking row follows for the
    /// model's next words.
    mutating func toolRequest(_ call: AgentToolCall) {
        guard isThinking, let since else { return }
        if case let .streaming(partial, _) = turns[turns.count - 1], !partial.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            turns[turns.count - 1] = .text(partial)
        } else {
            turns.removeLast()
        }
        turns.append(.tool(call))
        turns.append(.thinking(profile: "agent", since: since))
    }

    /// The command ran, was denied, or could not run.
    mutating func toolResult(id: String, status: AgentToolCall.Status, command: String?, output: String?) {
        guard let i = turns.lastIndex(where: {
            if case let .tool(c) = $0 {
                c.id == id
            } else {
                false
            }
        }),
            case var .tool(call) = turns[i] else { return }
        call.status = status
        if let command {
            call.command = command
        }
        call.output = output
        turns[i] = .tool(call)
    }

    /// True when the request in flight already ran a tool: the final
    /// answer then shows only its last segment, the rest is in place.
    private var hasToolTurnsInFlight: Bool {
        for turn in turns.reversed() {
            switch turn {
            case .user: return false
            case .tool: return true
            default: continue
            }
        }
        return false
    }

    mutating func answer(_ answer: AgentAnswer) {
        if case let .thinking(_, since) = turns.last {
            let secs = Date().timeIntervalSince(since)
            if secs >= 1 {
                turns.insert(.thought(seconds: secs), at: turns.count - 1)
            }
        }
        var shown = answer
        if hasToolTurnsInFlight {
            shown.transcript = answer.text
            if case let .streaming(partial, _) = turns.last {
                shown.text = partial
            } else {
                shown.text = ""
            }
        }
        replaceThinking(with: .answer(shown))
    }

    mutating func fail(_ message: String) {
        replaceThinking(with: .failure(message))
    }

    private mutating func replaceThinking(with turn: AgentTurn) {
        if isThinking {
            turns[turns.count - 1] = turn
        } else {
            turns.append(turn)
        }
        request = nil
    }

    /// Prior turns as `ai.ask` expects them: a question counts only once it
    /// has an answer, so a refused or in-flight prompt is never resent as
    /// a second consecutive user turn.
    var history: [AgentHistoryTurn] {
        var out: [AgentHistoryTurn] = []
        var pending: String?
        for turn in turns {
            switch turn {
            case let .user(text):
                pending = text
            case let .answer(answer):
                if let question = pending {
                    out.append(AgentHistoryTurn(role: "user", text: question))
                    out.append(AgentHistoryTurn(role: "assistant", text: answer.transcript ?? answer.text))
                }
                pending = nil
            case .text, .tool, .thought:
                break
            case .thinking, .streaming, .failure:
                pending = nil
            }
        }
        return out
    }

    /// Sum of every priced answer; nil until one is priced.
    var totalCostUSD: Double? {
        let priced = turns.compactMap { turn -> Double? in
            if case let .answer(a) = turn {
                return a.costUSD
            }
            return nil
        }
        return priced.isEmpty ? nil : priced.reduce(0, +)
    }

    var totalTokens: (input: Int, output: Int) {
        turns.reduce(into: (0, 0)) { acc, turn in
            if case let .answer(a) = turn {
                acc.0 += a.inputTokens
                acc.1 += a.outputTokens
            }
        }
    }
}

struct AgentHistoryTurn: Equatable, Sendable, Encodable {
    let role: String
    let text: String
}

/// `ai.ask` parameters. `stream` makes the reply `{ request }` and the
/// answer arrive as `ai.chunk`/`ai.done`/`ai.error` notifications.
struct AgentAskParams: Encodable, Sendable {
    let prompt: String
    let feature: String
    let session: String?
    let history: [AgentHistoryTurn]
    var stream = true
    /// The tool loop: the model may ask to run commands through the inbox.
    var agent = false
}

/// The `{ request }` reply of a streaming `ai.ask`.
struct AgentRequestReply: Decodable, Sendable {
    let request: String
}

struct AgentUsage: Decodable, Sendable {
    let inputTokens: Int
    let outputTokens: Int

    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
    }
}

/// `ai.ask` reply. `stop` is the provider's stop reason; unknown shapes
/// decode as "other" rather than failing the whole reply.
struct AgentAskReply: Decodable, Sendable {
    let text: String
    let profile: String
    let model: String
    let usage: AgentUsage
    let costUSDEstimate: Double?
    let redactions: Int
    let stop: JSONValue?

    enum CodingKeys: String, CodingKey {
        case text, profile, model, usage, redactions, stop
        case costUSDEstimate = "cost_usd_estimate"
    }

    func answer(seconds: Double) -> AgentAnswer {
        AgentAnswer(
            text: text, profile: profile, model: model,
            inputTokens: usage.inputTokens, outputTokens: usage.outputTokens,
            costUSD: costUSDEstimate, redactions: redactions,
            truncated: stop?.stringValue == "max_tokens", seconds: seconds
        )
    }
}
