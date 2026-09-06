// Cold path to vtermd: one JSON-RPC call per invocation through
// `vt_daemon_call` (ADR-0002 amendment). Blocking — never call on the
// render path. Replies are decoded with Codable so the wire contract stays
// the one the `vterm` CLI already exercises.

import CVambiantTerm
import Foundation

struct DaemonError: Error, CustomStringConvertible {
    let message: String
    var description: String {
        message
    }
}

struct SessionInfo: Decodable, Sendable {
    let id: String
    let name: String
    /// `[cols, rows]` on the wire.
    let size: [UInt16]
    let state: String?
    let degraded: String?
    let cwd: String?

    var cols: UInt16 {
        size.first ?? 0
    }

    var rows: UInt16 {
        size.count > 1 ? size[1] : 0
    }
}

struct DaemonStatus: Decodable, Sendable {
    let version: String?
    let pid: Int32?
    let sessions: Int
}

final class DaemonClient: Sendable {
    let socket: String

    init(socket: String) {
        self.socket = socket
    }

    /// The daemon socket for this user, honouring `VAMBIANT_TERM_RUNTIME`
    /// exactly as the CLI and the daemon do.
    static func defaultSocket() -> String {
        guard let raw = vt_default_socket() else { return "" }
        defer { vt_string_free(raw) }
        return String(cString: raw)
    }

    /// Raw call: JSON in, decoded `result` out.
    func call<T: Decodable>(_ method: String, params: (some Encodable)? = Int?.none) throws -> T {
        let data = try callRaw(method, params: params)
        do {
            return try JSONDecoder().decode(Reply<T>.self, from: data).result
        } catch {
            let body = String(data: data, encoding: .utf8) ?? "<non-utf8>"
            throw DaemonError(message: "\(method): unreadable reply (\(error)): \(body)")
        }
    }

    /// Fire-and-check: the result is ignored, errors surface.
    func invoke(_ method: String, params: (some Encodable)? = Int?.none) throws {
        _ = try callRaw(method, params: params)
    }

    private func callRaw(_ method: String, params: (some Encodable)?) throws -> Data {
        var paramsJSON: String?
        if let params {
            paramsJSON = try String(data: JSONEncoder().encode(params), encoding: .utf8)
        }
        guard let raw = vt_daemon_call(socket, method, paramsJSON) else {
            throw DaemonError(message: "\(method): vt_daemon_call returned null")
        }
        defer { vt_string_free(raw) }
        let data = Data(bytes: raw, count: strlen(raw))
        if let failure = try? JSONDecoder().decode(Failure.self, from: data) {
            throw DaemonError(message: "\(method): \(failure.error.message)")
        }
        return data
    }

    private struct Reply<T: Decodable>: Decodable { let result: T }
    private struct FailureBody: Decodable { let message: String }
    private struct Failure: Decodable { let error: FailureBody }
}

// MARK: - Session lifecycle

struct NewSessionRequest: Encodable {
    var name: String?
    var argv: [String] = []
    var cwd: String?
    var env: [[String]] = []
    var size: [UInt16]?
    /// `claude`, `codex`; nil is a plain shell under generic observation.
    var agent: String?
}

extension DaemonClient {
    func status() throws -> DaemonStatus {
        try call("daemon.status")
    }

    func newSession(cols: UInt16, rows: UInt16, cwd: String?, argv: [String] = [], agent: String? = nil) throws -> SessionInfo {
        try call("session.new", params: NewSessionRequest(argv: argv, cwd: cwd, size: [cols, rows], agent: agent))
    }

    func sessions() throws -> [SessionInfo] {
        try call("session.list")
    }

    /// Start `vtermd` if it is not answering, and wait for it. The binary
    /// is looked up next to this executable (inside the bundle), then via
    /// `VAMBIANT_TERM_VTERMD`; anything else is reported, not guessed.
    func ensureRunning() throws -> DaemonStatus {
        if let status = try? status() {
            return status
        }
        let candidates = [
            ProcessInfo.processInfo.environment["VAMBIANT_TERM_VTERMD"],
            Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("vtermd").path,
        ].compactMap(\.self).filter { FileManager.default.isExecutableFile(atPath: $0) }
        guard let bin = candidates.first else {
            let hint = "set VAMBIANT_TERM_VTERMD or bundle it next to the app executable"
            throw DaemonError(message: "vtermd is not running at \(socket) and no vtermd binary was found (\(hint))")
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: bin)
        process.arguments = ["--socket", socket]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            throw DaemonError(message: "cannot start \(bin): \(error)")
        }
        let deadline = Date().addingTimeInterval(5)
        while Date() < deadline {
            if let status = try? status() {
                return status
            }
            Thread.sleep(forTimeInterval: 0.02)
        }
        throw DaemonError(message: "started \(bin) (pid \(process.processIdentifier)) but \(socket) never answered")
    }
}
