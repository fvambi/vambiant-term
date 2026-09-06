// Pane-level actions that open app-wide surfaces: ⌃R in the editor opens
// the palette in its history scope (Warp's command search); ⌥⌘E shows the
// last request sent on this session's behalf, exactly as it left the
// machine (docs/05 §4.2, docs/06 §6).

import AppKit

extension PaneController {
    var windowController: TerminalWindowController? {
        container.window?.windowController as? TerminalWindowController
    }

    func wireAppActions() {
        container.input.editor.onHistorySearch = { [weak self] in self?.openHistorySearch() }
    }

    func openHistorySearch() {
        guard let controller = windowController else { return }
        (NSApp.delegate as? AppDelegate)?.showPalette(for: controller, pane: self, query: "h:")
    }

    /// `ai.payload.last` for this session, in a sheet; the daemon's refusal
    /// ("nothing has been sent") is shown as the body.
    func showLastPayload() {
        guard let window = container.window else { return }
        struct Params: Encodable {
            let session: String?
        }
        let sheet = (NSApp.delegate as? AppDelegate)?.payloadSheet ?? TextSheet()
        do {
            let v: JSONValue = try daemon.call("ai.payload.last", params: Params(session: session?.id))
            let header = [
                v[path: "at"]?.stringValue, v[path: "feature"]?.stringValue, v[path: "profile"]?.stringValue,
                v[path: "model"]?.stringValue, v[path: "redactions"]?.doubleValue.map { "\(Int($0)) redactions" },
            ].compactMap(\.self).joined(separator: " · ")
            let body = (try? JSONEncoder.pretty.encode(v[path: "request"] ?? .null)).flatMap { String(data: $0, encoding: .utf8) } ?? "\(v)"
            sheet.show(title: "Last payload sent", header: header, body: body, over: window)
        } catch {
            sheet.show(
                title: "Last payload sent",
                header: "nothing to show",
                body: "\(error)".replacingOccurrences(of: "ai.payload.last: ", with: ""),
                over: window
            )
        }
    }
}

extension JSONEncoder {
    static let pretty: JSONEncoder = {
        let e = JSONEncoder()
        e.outputFormatting = [.prettyPrinted, .sortedKeys]
        return e
    }()
}
