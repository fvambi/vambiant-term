// The pane: an NSView backed by a CAMetalLayer. A dirty signal from the
// viewer renders immediately when a frame slot is free (lowest latency for
// a lone keystroke); signals that land while two frames are in flight are
// coalesced by a CADisplayLink that pauses as soon as the grid is clean.
//
// Measured on 2026-09-05 (docs/07 M4): presenting a frame every tick while
// a pane is active, to keep a ProMotion panel at 120 Hz, made commit→present
// worse (32 ms vs 20 ms) because frames queued behind each other. So the
// view renders on change only.

import AppKit
import CVambiantTerm
@preconcurrency import Metal
import QuartzCore

struct FrameSummary: Sendable {
    let seq: UInt64
    let text: String
}

@MainActor
final class MetalGridView: NSView {
    let renderer: GridRenderer
    private let metalLayer = CAMetalLayer()
    private var displayLink: CADisplayLink?
    /// Frames committed but not yet finished on the GPU.
    private var inFlight = 0
    private static let maxInFlight = 2
    private var pending = false
    private var lastSeq: UInt64 = .max
    private var keymap = Keymap()
    private let banner = NSTextField(labelWithString: "")
    private(set) var cols: UInt16 = 0
    private(set) var rows: UInt16 = 0
    private var focused = false
    /// Self-screenshot (VAMBIANT_TERM_SCREENSHOT=<png>): `captureNext(to:)`
    /// renders one frame, reads its texture back and writes it out.
    private let screenshotEnabled = ProcessInfo.processInfo.environment["VAMBIANT_TERM_SCREENSHOT"] != nil
    private var screenshotPath: String?
    let padding = CGSize(width: 8, height: 6)

    var viewer: SessionViewer? {
        didSet {
            lastSeq = .max
            markDirty()
        }
    }

    /// Shell actions resolved from the keymap.
    var onAction: ((ShellAction) -> Void)?
    /// Grid size changed (cells); the owner forwards it to the daemon.
    var onResize: ((UInt16, UInt16) -> Void)?
    /// Every presented frame, when set (latency probe only — it allocates).
    var onPresented: (@Sendable (FrameSummary, CFTimeInterval) -> Void)?
    /// Every committed frame, when set (latency probe only).
    var onCommit: ((UInt64, CFTimeInterval) -> Void)?
    /// Every GPU-completed frame, when set (latency probe only).
    var onGPUDone: (@Sendable (CFTimeInterval) -> Void)?
    var onDisconnected: (() -> Void)?

    init(renderer: GridRenderer) {
        self.renderer = renderer
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        metalLayer.device = renderer.device
        metalLayer.pixelFormat = .bgra8Unorm
        metalLayer.framebufferOnly = !screenshotEnabled
        metalLayer.isOpaque = true
        metalLayer.presentsWithTransaction = false
        metalLayer.maximumDrawableCount = 3
        banner.isHidden = true
        banner.textColor = .white
        banner.backgroundColor = NSColor(red: 0.7, green: 0.2, blue: 0.2, alpha: 0.95)
        banner.drawsBackground = true
        banner.font = NSFont.systemFont(ofSize: 12, weight: .semibold)
        banner.alignment = .center
        addSubview(banner)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    override func makeBackingLayer() -> CALayer {
        metalLayer
    }

    override var acceptsFirstResponder: Bool {
        true
    }

    override var isFlipped: Bool {
        true
    }

    // MARK: Lifecycle

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        displayLink?.invalidate()
        displayLink = nil
        guard window != nil else { return }
        let link = displayLink(target: self, selector: #selector(tick))
        let hz = Float(window?.screen?.maximumFramesPerSecond ?? 60)
        link.preferredFrameRateRange = CAFrameRateRange(minimum: hz, maximum: hz, preferred: hz)
        link.isPaused = true
        link.add(to: .main, forMode: .common)
        displayLink = link
        updateBacking()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        updateBacking()
    }

    override func layout() {
        super.layout()
        banner.frame = CGRect(x: 0, y: 0, width: bounds.width, height: 22)
        updateBacking()
    }

    private func updateBacking() {
        let scale = window?.backingScaleFactor ?? 2
        metalLayer.contentsScale = scale
        do {
            try renderer.setScale(scale)
        } catch {
            NSLog("renderer: \(error)")
        }
        let size = CGSize(width: max(1, bounds.width * scale), height: max(1, bounds.height * scale))
        if metalLayer.drawableSize != size {
            metalLayer.drawableSize = size
        }
        let cell = renderer.cellSize
        let newCols = UInt16(max(2, floor((size.width - padding.width * 2 * scale) / cell.width)))
        let newRows = UInt16(max(1, floor((size.height - padding.height * 2 * scale) / cell.height)))
        if newCols != cols || newRows != rows {
            cols = newCols
            rows = newRows
            onResize?(cols, rows)
        }
        lastSeq = .max
        markDirty()
    }

    override func becomeFirstResponder() -> Bool {
        focused = true
        lastSeq = .max
        markDirty()
        return true
    }

    override func resignFirstResponder() -> Bool {
        focused = false
        lastSeq = .max
        markDirty()
        return true
    }

    // MARK: Drawing

    /// Diagnostics: write the next rendered frame to `path` and log the
    /// grid it was built from.
    func captureNext(to path: String) {
        screenshotPath = path
        if let viewer {
            let (seq, text): (UInt64, String) = viewer.withGrid { ($0.seq, $0.text()) }
            NSLog("screenshot: grid seq %llu:\n%@", seq, text)
        }
        lastSeq = .max
        markDirty()
    }

    /// Main-actor entry for the viewer's dirty signal.
    func markDirty() {
        if inFlight >= Self.maxInFlight {
            pending = true
            displayLink?.isPaused = false
            return
        }
        render()
    }

    @objc private func tick() {
        if inFlight >= Self.maxInFlight {
            return
        }
        if pending {
            pending = false
            render()
        } else if inFlight == 0 {
            displayLink?.isPaused = true
        }
    }

    private func render() {
        guard let viewer, window != nil, bounds.width > 0 else { return }
        let seq = viewer.seq
        let disconnected: Bool = viewer.withGrid { $0.disconnected }
        if disconnected, banner.isHidden {
            banner.stringValue = "daemon connection lost — the grid below is the last known state; reopen to reattach"
            banner.isHidden = false
            onDisconnected?()
        }
        if seq == lastSeq, !disconnected {
            return
        }
        guard let drawable = metalLayer.nextDrawable() else {
            pending = true
            return
        }
        let scale = metalLayer.contentsScale
        let origin = CGPoint(x: padding.width * scale, y: padding.height * scale)
        var summary: FrameSummary?
        let built: Bool = viewer.withGrid { view in
            var ok = renderer.build(view, origin: origin, focused: focused)
            if !ok {
                ok = renderer.build(view, origin: origin, focused: focused)
            }
            if onPresented != nil {
                summary = FrameSummary(seq: view.seq, text: view.text())
            }
            return ok
        }
        guard built else { return }
        lastSeq = seq
        inFlight += 1
        onCommit?(seq, CACurrentMediaTime())
        let presented = onPresented
        let frame = summary
        let gpuDone = onGPUDone
        var capture: (@Sendable (MTLTexture) -> Void)?
        if let path = screenshotPath {
            screenshotPath = nil
            capture = { texture in Screenshot.write(texture, to: path) }
        }
        renderer.encode(
            into: drawable,
            viewport: metalLayer.drawableSize,
            onGPUDone: { t in
                gpuDone?(t)
                DispatchQueue.main.async {
                    MainActor.assumeIsolated { self.frameFinished() }
                }
            },
            capture: capture
        ) { t in
            if let presented, let frame {
                presented(frame, t)
            }
        }
    }

    private func frameFinished() {
        inFlight -= 1
        if pending, inFlight < Self.maxInFlight {
            pending = false
            render()
        }
    }

    // MARK: Input

    override func keyDown(with event: NSEvent) {
        switch keymap.resolve(Keymap.chord(from: event)) {
        case .prefixArmed:
            return
        case let .action(action):
            onAction?(action)
        case .passthrough:
            guard let viewer else { return }
            if !viewer.send(key: KeyTranslator.event(from: event)) {
                markDirty()
            }
        }
    }

    override func flagsChanged(with event: NSEvent) {
        // Modifier-only events carry nothing a terminal program can use
        // until kitty's report-all-keys is on; the daemon owns that mode.
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        // Keep ⌘-chords out of the menu bar when the keymap binds them;
        // unbound ones (⌘Q, ⌘V…) fall through to the menu as usual.
        let chord = Keymap.chord(from: event)
        guard chord.command, Keymap.macos[chord] != nil else { return false }
        keyDown(with: event)
        return true
    }

    @objc func paste(_ sender: Any?) {
        guard let viewer, let text = NSPasteboard.general.string(forType: .string) else { return }
        // Bracketed paste is a terminal mode the daemon owns; raw bytes are
        // correct until `session.paste` exists (M5).
        viewer.send(text: text)
    }

    @objc func copy(_ sender: Any?) {
        // Selection arrives in M5; until then copying is what the shell
        // program itself puts on the clipboard.
        NSSound.beep()
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
    }
}
