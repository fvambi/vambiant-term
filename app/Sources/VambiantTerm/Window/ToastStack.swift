// Warp's toasts (12 §G3): at most two cards at the top right of the
// window for events in panes that are not on screen; each fades after a
// few seconds, hovering pauses the clock, clicking goes to the session.

import AppKit

@MainActor
final class ToastStack: NSView {
    var onOpen: ((Note) -> Void)?
    private var cards: [ToastCard] = []
    static let width: CGFloat = 340
    static let maxShown = 2

    override var isFlipped: Bool {
        true
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        return hit === self ? nil : hit // the empty area lets clicks through
    }

    func show(_ note: Note, theme: Theme) {
        if let existing = cards.first(where: { $0.note.id == note.id }) {
            existing.update(note)
            existing.restart()
            relayout()
            return
        }
        let card = ToastCard(note: note, theme: theme)
        card.onClose = { [weak self, weak card] in
            guard let self, let card else { return }
            remove(card)
        }
        card.onOpen = { [weak self] note in self?.onOpen?(note) }
        cards.insert(card, at: 0)
        addSubview(card)
        while cards.count > Self.maxShown, let last = cards.last {
            remove(last)
        }
        relayout()
    }

    private func remove(_ card: ToastCard) {
        cards.removeAll { $0 === card }
        card.removeFromSuperview()
        relayout()
    }

    private func relayout() {
        var y: CGFloat = 8
        for card in cards {
            card.frame = CGRect(x: bounds.width - Self.width - 12, y: y, width: Self.width, height: ToastCard.height)
            y += ToastCard.height + 8
        }
    }

    override func layout() {
        super.layout()
        relayout()
    }
}

@MainActor
final class ToastCard: NSView {
    private(set) var note: Note
    var onClose: (() -> Void)?
    var onOpen: ((Note) -> Void)?
    private let title = NSTextField(labelWithString: "")
    private let body = NSTextField(labelWithString: "")
    private var timer: Timer?
    private var tracking: NSTrackingArea?
    static let height: CGFloat = 58
    static let lifetime: TimeInterval = 6

    init(note: Note, theme: Theme) {
        self.note = note
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = theme.background.mixed(with: theme.foreground, 0.14).nsColor.cgColor
        layer?.borderWidth = 1
        let tint: RGBA = switch note.kind {
        case .error: theme.palette[1]
        case .request: theme.palette[3]
        case .complete: theme.palette[2]
        case .info: theme.palette[4]
        }
        layer?.borderColor = tint.nsColor.withAlphaComponent(0.6).cgColor
        title.font = NSFont.systemFont(ofSize: 12, weight: .semibold)
        title.textColor = tint.nsColor
        body.font = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
        body.textColor = theme.foreground.nsColor
        body.lineBreakMode = .byTruncatingMiddle
        for v in [title, body] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            title.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            body.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: title.trailingAnchor),
            body.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 3),
        ])
        update(note)
        restart()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    func update(_ note: Note) {
        self.note = note
        title.stringValue = note.title
        body.stringValue = note.body
    }

    func restart() {
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: Self.lifetime, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.onClose?() }
        }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking {
            removeTrackingArea(tracking)
        }
        let area = NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeInKeyWindow], owner: self)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) {
        timer?.invalidate() // hover pauses the clock
    }

    override func mouseExited(with event: NSEvent) {
        restart()
    }

    override func mouseDown(with event: NSEvent) {
        onOpen?(note)
        onClose?()
    }
}
