// Keystroke → presented-frame latency, in-process. Not a typometer: it
// starts the clock when the key event enters the view and stops it when
// the frame carrying the echoed glyph is handed to the compositor
// (`CAMetalDrawable.presentedTime`). Scan-out adds up to one refresh on
// top and is not measured here; the report says so.
//
// Enabled with VAMBIANT_TERM_LATENCY_PROBE=<n>; prints JSON and exits.

import AppKit
import CVambiantTerm
import Foundation

@MainActor
final class LatencyProbe {
    private let pane: PaneController
    private let keystrokes: Int
    private var sent: [(count: Int, at: CFTimeInterval)] = []
    private var samples: [Double] = []
    private let state = ProbeState()
    private var timer: Timer?
    private var settledAt: CFTimeInterval?

    init(pane: PaneController, keystrokes: Int) {
        self.pane = pane
        // The count is taken over the whole grid, so stay well inside it.
        self.keystrokes = min(keystrokes, Int(pane.view.cols) * max(1, Int(pane.view.rows) - 4))
    }

    func start() {
        let state = self.state
        pane.view.onPresented = { frame, at in
            state.record(count: frame.text.filter { $0 == "x" }.count, at: at)
        }
        pane.dirtyHook.set { at in state.recordDirty(at) }
        pane.view.onCommit = { _, at in state.recordCommit(at) }
        pane.view.onGPUDone = { at in state.recordGPU(at) }
        // Let the shell print its prompt first.
        timer = Timer.scheduledTimer(withTimeInterval: 1.5, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.typeNext() }
        }
    }

    private func typeNext() {
        guard sent.count < keystrokes else { finish()
            return
        }
        let n = sent.count + 1
        var ev = VtKeyEvent()
        ev.key = 43 // X
        ev.utf8.0 = UInt8(ascii: "x")
        ev.utf8_len = 1
        ev.unshifted = UInt32(UInt8(ascii: "x"))
        let t0 = CACurrentMediaTime()
        pane.viewer?.send(key: ev)
        sent.append((n, t0))
        timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.typeNext() }
        }
    }

    private func finish() {
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.report() }
        }
    }

    private func report() {
        let frames = state.snapshot()
        let dirties = state.dirtySnapshot()
        let commits = state.commitSnapshot()
        let gpus = state.gpuSnapshot()
        var commitToGPU: [Double] = []
        var latencies: [Double] = []
        var toDirty: [Double] = []
        var dirtyToCommit: [Double] = []
        var commitToPresent: [Double] = []
        for (count, at) in sent {
            guard let frame = frames.first(where: { $0.count >= count }) else { continue }
            latencies.append((frame.at - at) * 1000)
            guard let d = dirties.first(where: { $0 > at }), let c = commits.first(where: { $0 > d }) else { continue }
            toDirty.append((d - at) * 1000)
            dirtyToCommit.append((c - d) * 1000)
            commitToPresent.append((frame.at - c) * 1000)
            if let g = gpus.first(where: { $0 > c }) {
                commitToGPU.append((g - c) * 1000)
            }
        }
        latencies.sort()
        func pct(_ p: Double, _ v: [Double]? = nil) -> Double {
            let v = (v ?? latencies).sorted()
            guard !v.isEmpty else { return .nan }
            let i = min(v.count - 1, Int((Double(v.count) * p).rounded(.up)) - 1)
            return v[max(0, i)]
        }
        let hz = NSScreen.main?.maximumFramesPerSecond ?? 0
        let out: [String: Any] = [
            "keystrokes": sent.count,
            "matched": latencies.count,
            "p50_ms": pct(0.5), "p95_ms": pct(0.95), "p99_ms": pct(0.99),
            "max_ms": latencies.last ?? .nan,
            "stages_p50_ms": [
                "key_to_dirty": pct(0.5, toDirty),
                "dirty_to_commit": pct(0.5, dirtyToCommit),
                "commit_to_present": pct(0.5, commitToPresent),
                "commit_to_gpu_done": pct(0.5, commitToGPU),
            ],
            "stages_p99_ms": [
                "key_to_dirty": pct(0.99, toDirty),
                "dirty_to_commit": pct(0.99, dirtyToCommit),
                "commit_to_present": pct(0.99, commitToPresent),
            ],
            "display_max_hz": hz,
            "measures": "key event entering the view → frame with the echoed glyph presented to the compositor; "
                + "scan-out (≤ 1 refresh) not included",
            "font": pane.view.renderer.fonts.familyName,
            "cols": pane.view.cols, "rows": pane.view.rows,
        ]
        let data = try! JSONSerialization.data(withJSONObject: out, options: [.sortedKeys, .prettyPrinted])
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data("\n".utf8))
        exit(0)
    }
}

/// Frame log written from the presentation callback thread.
final class ProbeState: @unchecked Sendable {
    private var frames: [(count: Int, at: CFTimeInterval)] = []
    private var dirties: [CFTimeInterval] = []
    private var commits: [CFTimeInterval] = []
    private var gpus: [CFTimeInterval] = []
    private let lock = NSLock()

    func recordGPU(_ at: CFTimeInterval) {
        lock.lock()
        defer { lock.unlock() }
        gpus.append(at)
    }

    func gpuSnapshot() -> [CFTimeInterval] {
        lock.lock()
        defer { lock.unlock() }
        return gpus
    }

    func recordDirty(_ at: CFTimeInterval) {
        lock.lock()
        defer { lock.unlock() }
        dirties.append(at)
    }

    func recordCommit(_ at: CFTimeInterval) {
        lock.lock()
        defer { lock.unlock() }
        commits.append(at)
    }

    func dirtySnapshot() -> [CFTimeInterval] {
        lock.lock()
        defer { lock.unlock() }
        return dirties
    }

    func commitSnapshot() -> [CFTimeInterval] {
        lock.lock()
        defer { lock.unlock() }
        return commits
    }

    func record(count: Int, at: CFTimeInterval) {
        lock.lock()
        defer { lock.unlock() }
        frames.append((count, at))
    }

    func snapshot() -> [(count: Int, at: CFTimeInterval)] {
        lock.lock()
        defer { lock.unlock() }
        return frames
    }
}
