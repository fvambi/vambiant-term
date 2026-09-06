// Daemon broadcasts for this pane's session, and the rename that the
// daemon answers with the session record.

import AppKit

extension PaneController {
    /// Daemon broadcasts; only this pane's session is acted on.
    func handle(event method: String, params: JSONValue) {
        guard let session, params[path: "id"]?.stringValue == session.id else { return }
        switch method {
        case "session.block":
            if var block = Block.parse(item: params) {
                block.cwd = cwd
                blocks.append(block)
                onChange?()
                if block.failed {
                    suggestCorrection()
                }
            }
        case "session.changed":
            sessionState = params[path: "state"]?.stringValue
            agentKind = params[path: "agent"]?.stringValue
            if let name = params[path: "name"]?.stringValue {
                sessionName = name
            }
            onChange?()
        case "session.event" where params[path: "event.kind"]?.stringValue == "bell":
            onBell?()
        case "session.event" where params[path: "event.kind"]?.stringValue == "blocks_degraded":
            blocks.degraded = params[path: "event.reason"]?.stringValue ?? "shell-integration marks are corrupted"
        case "session.event" where params[path: "event.kind"]?.stringValue == "prompt":
            setPhase(.prompt)
        case "session.event" where params[path: "event.kind"]?.stringValue == "command_started":
            setPhase(.running)
        case "session.event" where params[path: "event.kind"]?.stringValue == "pwd":
            if let path = params[path: "event.path"]?.stringValue {
                setCwd(path)
            }
        case "session.block_changed":
            if let seq = params[path: "seq"]?.doubleValue, let on = params[path: "bookmarked"]?.boolValue {
                blocks.setBookmark(seq: Int64(seq), on: on)
            }
        case "ai.chunk", "ai.done", "ai.error", "ai.tool_request", "ai.tool_result":
            handleAgent(event: method, params: params)
        case "agent.event":
            if let event = params[path: "event"] {
                activity.apply(event)
            }
        default:
            break
        }
    }

    /// `session.rename`; the daemon answers with the record.
    func rename(to name: String) {
        guard let session else { return }
        struct Params: Encodable {
            let id: String
            let name: String
        }
        do {
            let info: SessionInfo = try daemon.call("session.rename", params: Params(id: session.id, name: name))
            sessionName = info.name
            onChange?()
        } catch {
            container.input.setHint("rename failed: \("\(error)".replacingOccurrences(of: "session.rename: ", with: ""))")
        }
    }
}
