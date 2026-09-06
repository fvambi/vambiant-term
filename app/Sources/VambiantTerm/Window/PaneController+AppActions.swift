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

    /// After a failed command: the daemon's correction, as ghost text in
    /// the empty editor with the reason in the hint (Warp's "did you mean").
    func suggestCorrection() {
        guard warpMode, let session else { return }
        struct Params: Encodable {
            let session: String
        }
        struct Correction: Decodable {
            let command: String
            let rule: String
            let explanation: String
        }
        struct Reply: Decodable {
            let corrections: [Correction]
        }
        let daemon = self.daemon
        let id = session.id
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let reply: Reply? = try? daemon.call("correct.suggest", params: Params(session: id))
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, self.session?.id == id, let first = reply?.corrections.first,
                          self.container.input.editor.string.isEmpty else { return }
                    self.container.input.editor.correction = first.command
                    self.container.input.setHint(Autosuggest.correctionHint(command: first.command, explanation: first.explanation))
                }
            }
        }
    }

    /// `path.executables` for the editor's unknown-command underline, at
    /// most once a minute; the daemon rescans PATH on the same cadence.
    func refreshKnownCommands() {
        if let at = knownCommandsFetchedAt, Date().timeIntervalSince(at) < 60 {
            return
        }
        knownCommandsFetchedAt = Date()
        let daemon = self.daemon
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let names: [String]? = try? daemon.call("path.executables")
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, let names else { return }
                    self.container.input.editor.knownCommands = Set(names)
                }
            }
        }
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
