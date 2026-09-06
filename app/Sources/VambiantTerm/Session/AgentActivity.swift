// What a supervised agent is doing, folded from the daemon's `agent.event`
// broadcasts (12 §G11, docs/06 §4): the tool call in flight, the last one
// that finished, the task list from TodoWrite, thinking, files touched,
// and a log. Pure, so the folding is tested without a daemon.

import Foundation

struct AgentTask: Equatable, Sendable {
    enum Status: String, Equatable, Sendable {
        case pending, inProgress = "in_progress", completed
    }

    let content: String
    let status: Status

    var glyph: String {
        switch status {
        case .pending: "○"
        case .inProgress: "●"
        case .completed: "✔"
        }
    }
}

struct AgentToolCallSummary: Equatable, Sendable {
    let id: String
    let name: String
    let summary: String
    let startedAt: Date
    var ok: Bool?
    var durationMs: UInt64?

    /// `Bash · cargo test`, `Edit · src/main.rs`, `TodoWrite · 5 tasks`.
    static func summarise(name: String, input: JSONValue) -> String {
        switch name {
        case "Bash": input[path: "command"]?.stringValue ?? ""
        case "Edit", "Write", "Read", "NotebookEdit": input[path: "file_path"]?.stringValue.map(short) ?? ""
        case "Grep", "Glob": input[path: "pattern"]?.stringValue ?? ""
        case "TodoWrite": "\(input[path: "todos"]?.arrayValue?.count ?? 0) tasks"
        case "WebFetch", "WebSearch": input[path: "url"]?.stringValue ?? input[path: "query"]?.stringValue ?? ""
        case "Task", "Agent": input[path: "description"]?.stringValue ?? ""
        default: ""
        }
    }

    static func short(_ path: String) -> String {
        let parts = path.split(separator: "/")
        return parts.count > 2 ? "…/" + parts.suffix(2).joined(separator: "/") : path
    }

    var line: String {
        let what = summary.isEmpty ? name : "\(name) · \(summary)"
        if let durationMs {
            let secs = Double(durationMs) / 1000
            return "\(ok == false ? "✕" : "✔") \(what)" + (secs >= 0.1 ? String(format: " · %.1fs", secs) : "")
        }
        return "⚙ \(what)"
    }
}

struct AgentActivity: Equatable, Sendable {
    private(set) var current: AgentToolCallSummary?
    private(set) var lastDone: AgentToolCallSummary?
    private(set) var tasks: [AgentTask] = []
    private(set) var thinking = false
    private(set) var files: [String] = []
    private(set) var log: [String] = []
    private(set) var lastText: String?
    static let logCap = 200

    var isEmpty: Bool {
        current == nil && lastDone == nil && tasks.isEmpty && !thinking && lastText == nil
    }

    /// The strip's first line: the call in flight, else thinking, else the last result.
    var headline: String? {
        if let current {
            return current.line
        }
        if thinking {
            return "◐ thinking…"
        }
        if let lastDone {
            return lastDone.line
        }
        if let lastText {
            return "💬 \(lastText.split(separator: "\n").first.map(String.init) ?? lastText)"
        }
        return nil
    }

    /// The strip's second line: `✔ 2/5 tasks · ● Write the parser`.
    var taskLine: String? {
        guard !tasks.isEmpty else { return nil }
        let done = tasks.filter { $0.status == .completed }.count
        var line = "\(done)/\(tasks.count) tasks"
        if let active = tasks.first(where: { $0.status == .inProgress }) {
            line += " · ● \(active.content)"
        } else if let next = tasks.first(where: { $0.status == .pending }) {
            line += " · ○ \(next.content)"
        }
        return line
    }

    /// One `agent.event` payload (`{ type, … }`), as the daemon tags it.
    mutating func apply(_ event: JSONValue, at now: Date = Date()) {
        guard let type = event[path: "type"]?.stringValue else { return }
        switch type {
        case "tool_call_start":
            let name = event[path: "name"]?.stringValue ?? "tool"
            let call = AgentToolCallSummary(
                id: event[path: "id"]?.stringValue ?? "", name: name,
                summary: AgentToolCallSummary.summarise(name: name, input: event[path: "input"] ?? .null), startedAt: now
            )
            current = call
            thinking = false
            if name == "TodoWrite", let todos = event[path: "input.todos"]?.arrayValue {
                tasks = todos.compactMap { t in
                    guard let content = t[path: "content"]?.stringValue else { return nil }
                    return AgentTask(
                        content: content,
                        status: AgentTask.Status(rawValue: t[path: "status"]?.stringValue ?? "pending") ?? .pending
                    )
                }
            }
            append("⚙ \(call.summary.isEmpty ? name : "\(name) · \(call.summary)")", at: now)
        case "tool_call_end":
            let id = event[path: "id"]?.stringValue ?? ""
            var finished = current?.id == id ? current : AgentToolCallSummary(id: id, name: "tool", summary: "", startedAt: now)
            finished?.ok = event[path: "ok"]?.boolValue ?? true
            finished?.durationMs = event[path: "duration_ms"]?.doubleValue.map { UInt64($0) }
            lastDone = finished
            if current?.id == id {
                current = nil
            }
            if let finished {
                append(finished.line, at: now)
            }
        case "thinking":
            thinking = true
            if let summary = event[path: "summary"]?.stringValue, !summary.isEmpty {
                append("◐ \(summary)", at: now)
            }
        case "assistant_text":
            if event[path: "streaming"]?.boolValue != true, let text = event[path: "text"]?.stringValue, !text.isEmpty {
                lastText = text
                lastDone = nil // the text is newer than the last result
                thinking = false
                append("💬 \(text.split(separator: "\n").first.map(String.init) ?? text)", at: now)
            }
        case "file_changed":
            if let path = event[path: "path"]?.stringValue {
                files.removeAll { $0 == path }
                files.insert(path, at: 0)
                if files.count > 5 {
                    files.removeLast(files.count - 5)
                }
                append("✎ \(AgentToolCallSummary.short(path))", at: now)
            }
        case "state_changed":
            if let to = event[path: "to"]?.stringValue {
                thinking = to == "thinking"
                if to == "stopped" || to == "crashed" {
                    current = nil
                }
                append("→ \(to)", at: now)
            }
        case "subagent_start":
            append("⇢ subagent \(event[path: "kind"]?.stringValue ?? "")", at: now)
        case "error":
            append("✕ \(event[path: "message"]?.stringValue ?? "error")", at: now)
        default:
            break
        }
    }

    private mutating func append(_ line: String, at now: Date) {
        let stamp = Self.clock.string(from: now)
        log.append("\(stamp)  \(line)")
        if log.count > Self.logCap {
            log.removeFirst(log.count - Self.logCap)
        }
    }

    private static let clock: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss"
        return f
    }()
}
