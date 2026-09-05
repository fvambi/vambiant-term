// Hot path: a Swift owner for one `VtViewer`. The grid is read by pointer
// between acquire and release; nothing is copied. The dirty callback fires
// on the viewer's Rust thread and is relayed to the main actor here — the
// only place that crossing happens.

import CVambiantTerm
import Foundation

struct ViewerError: Error, CustomStringConvertible {
    let message: String
    var description: String {
        message
    }
}

/// Relay object handed to Rust as the callback context. Rust calls
/// `fire` from its reader thread; the closure must be `@Sendable`.
final class DirtyRelay: @unchecked Sendable {
    let onDirty: @Sendable () -> Void
    init(onDirty: @escaping @Sendable () -> Void) {
        self.onDirty = onDirty
    }
}

private let dirtyTrampoline: @convention(c) (UnsafeMutableRawPointer?) -> Void = { ctx in
    guard let ctx else { return }
    Unmanaged<DirtyRelay>.fromOpaque(ctx).takeUnretainedValue().onDirty()
}

final class SessionViewer {
    let sessionID: String
    private var raw: OpaquePointer
    private let relay: DirtyRelay

    /// Attaches to `sessionID` on `socket`. `onDirty` runs on an arbitrary
    /// thread every time the grid changed or the connection dropped.
    init(socket: String, sessionID: String, onDirty: @escaping @Sendable () -> Void) throws {
        let relay = DirtyRelay(onDirty: onDirty)
        let ctx = Unmanaged.passUnretained(relay).toOpaque()
        guard let ptr = vt_viewer_attach(socket, sessionID, dirtyTrampoline, ctx) else {
            let reason = vt_viewer_last_error().map { String(cString: $0) } ?? "unknown"
            throw ViewerError(message: reason)
        }
        self.sessionID = sessionID
        self.raw = ptr
        self.relay = relay
    }

    deinit {
        if !vt_viewer_free(raw) {
            // Freed while acquired: a programming error, and leaking the
            // viewer is the only memory-safe option.
            NSLog("vt_viewer_free refused: viewer for \(sessionID) still acquired; leaking it")
        }
    }

    /// Sequence number of the last applied delta, without locking.
    var seq: UInt64 {
        vt_viewer_seq(raw)
    }

    /// Runs `body` with the grid locked. Keep it short: the reader thread
    /// waits for the whole duration.
    func withGrid<T>(_ body: (VtGridView) throws -> T) rethrows -> T {
        let view = vt_viewer_acquire(raw)
        defer { vt_viewer_release(raw) }
        return try body(view)
    }

    @discardableResult
    func send(key: VtKeyEvent) -> Bool {
        vt_viewer_send_key(raw, key)
    }

    @discardableResult
    func send(bytes: [UInt8]) -> Bool {
        bytes.withUnsafeBufferPointer { buf in
            vt_viewer_send_bytes(raw, buf.baseAddress, buf.count)
        }
    }

    @discardableResult
    func send(text: String) -> Bool {
        send(bytes: Array(text.utf8))
    }

    @discardableResult
    func resize(cols: UInt16, rows: UInt16) -> Bool {
        vt_viewer_resize(raw, cols, rows)
    }
}

extension VtGridView {
    /// The grid as text, one line per row, trailing blanks trimmed. Test
    /// and diagnostic use only — it allocates.
    func text() -> String {
        guard let cells, cols > 0 else { return "" }
        let w = Int(cols)
        var lines: [String] = []
        for r in 0 ..< Int(rows) {
            var line = ""
            for c in 0 ..< w {
                let cell = cells[r * w + c]
                if cell.attrs & UInt16(Attrs.wideSpacer) != 0 {
                    continue
                }
                line.unicodeScalars.append(Unicode.Scalar(cell.ch) ?? " ")
            }
            while line.last == " " {
                line.removeLast()
            }
            lines.append(line)
        }
        return lines.joined(separator: "\n")
    }
}

/// `vt_core::cell::Attrs` bits, mirrored (the header carries only the
/// integer field).
enum Attrs {
    static let bold: UInt16 = 1 << 0
    static let italic: UInt16 = 1 << 1
    static let underline: UInt16 = 1 << 2
    static let strikeout: UInt16 = 1 << 3
    static let inverse: UInt16 = 1 << 4
    static let dim: UInt16 = 1 << 5
    static let hidden: UInt16 = 1 << 6
    static let wide: UInt16 = 1 << 7
    static let wideSpacer: UInt16 = 1 << 8
}
