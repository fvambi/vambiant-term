// The Settings window's model: one snapshot of `config.get`, edits sent
// straight to the daemon (which validates and writes), and a reload after
// each so the window always shows the file's truth, never a local guess.

import AppKit
import Foundation
import Observation

@MainActor
@Observable
final class ConfigModel {
    let daemon: DaemonClient
    private(set) var snapshot: ConfigSnapshot?
    /// The last daemon refusal, verbatim; cleared by the next success.
    var lastError: String?
    /// Fires after every successful reload with the fresh snapshot.
    var onChange: ((ConfigSnapshot) -> Void)?

    init(daemon: DaemonClient) {
        self.daemon = daemon
    }

    func reload() {
        do {
            let fresh: ConfigSnapshot = try daemon.call("config.get")
            snapshot = fresh
            onChange?(fresh)
        } catch {
            lastError = "\(error)"
        }
    }

    func value(_ path: String) -> JSONValue? {
        snapshot?.config[path: path]
    }

    func defaultValue(_ path: String) -> JSONValue? {
        snapshot?.defaults[path: path]
    }

    func set(_ path: String, _ value: JSONValue) {
        perform { try daemon.invoke("config.set", params: SetParams(key: path, value: value)) }
    }

    func reset(_ path: String) {
        guard let d = defaultValue(path) else { return }
        set(path, d)
    }

    func bind(chord: String, action: String) {
        perform { try daemon.invoke("config.keymap.set", params: BindParams(chord: chord, action: action)) }
    }

    func unbind(chord: String) {
        perform { try daemon.invoke("config.keymap.set", params: BindParams(chord: chord, action: nil)) }
    }

    func save(theme: ThemeFile) {
        perform { try daemon.invoke("config.theme.save", params: ThemeParams(theme: theme)) }
    }

    /// Imports a theme file (Warp, Ghostty, Alacritty, iTerm2, base16);
    /// returns the saved name.
    @discardableResult
    func importTheme(path: String) -> String? {
        struct Params: Encodable {
            let path: String
        }
        struct Reply: Decodable {
            let name: String
            let warnings: [String]
        }
        var imported: String?
        perform {
            let reply: Reply = try daemon.call("config.theme.import", params: Params(path: path))
            imported = reply.name
            if !reply.warnings.isEmpty {
                lastError = "imported \(reply.name) with warnings: " + reply.warnings.joined(separator: "; ")
            }
        }
        return imported
    }

    func revealConfig() {
        guard let p = snapshot?.paths.config else { return }
        if !FileManager.default.fileExists(atPath: p) {
            FileManager.default.createFile(atPath: p, contents: Data("# Vambiant Term — see docs/09\n".utf8))
        }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: p)])
    }

    private func perform(_ body: () throws -> Void) {
        do {
            try body()
            lastError = nil
        } catch {
            lastError = "\(error)"
        }
        reload()
    }
}
