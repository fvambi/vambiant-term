// Daemon broadcasts (config.changed, session.changed, …) delivered to the
// main actor. One connection per app; the callback runs on the
// subscription's Rust thread and is hopped here.

import CVambiantTerm
import Foundation

final class EventRelay: @unchecked Sendable {
    let handler: @Sendable (String, String) -> Void
    init(handler: @escaping @Sendable (String, String) -> Void) {
        self.handler = handler
    }
}

private let eventTrampoline: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?)
    -> Void = { ctx, method, params in
        guard let ctx, let method else { return }
        let relay = Unmanaged<EventRelay>.fromOpaque(ctx).takeUnretainedValue()
        relay.handler(String(cString: method), params.map { String(cString: $0) } ?? "")
    }

final class EventStream {
    private var raw: OpaquePointer?
    private let relay: EventRelay

    /// `onEvent(method, paramsJSON)` on the main actor; method
    /// "disconnected" marks the end.
    init(socket: String, onEvent: @escaping @MainActor (String, String) -> Void) throws {
        relay = EventRelay { method, params in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { onEvent(method, params) }
            }
        }
        let ctx = Unmanaged.passUnretained(relay).toOpaque()
        guard let ptr = vt_events_subscribe(socket, eventTrampoline, ctx) else {
            throw ViewerError(message: vt_viewer_last_error().map { String(cString: $0) } ?? "unknown")
        }
        raw = ptr
    }

    deinit {
        vt_events_free(raw)
    }
}
