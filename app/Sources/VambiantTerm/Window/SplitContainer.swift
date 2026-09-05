// Splits as nested NSSplitViews. A leaf holds one pane; a branch holds
// two children. Focus moves geometrically (docs/06 §7 "Focus pane"), and
// zoom hides every pane but the focused one.

import AppKit

@MainActor
final class SplitContainer: NSView {
    private(set) var panes: [PaneController] = []
    private var root: NSView?
    private var zoomed: PaneController?
    private var savedFrames: [ObjectIdentifier: NSView] = [:]

    var onFocusChange: ((PaneController) -> Void)?

    init(initial: PaneController) {
        super.init(frame: .zero)
        autoresizingMask = [.width, .height]
        install(initial.view)
        panes = [initial]
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    private func install(_ view: NSView) {
        root?.removeFromSuperview()
        root = view
        view.frame = bounds
        view.autoresizingMask = [.width, .height]
        addSubview(view)
    }

    var focused: PaneController? {
        guard let responder = window?.firstResponder as? MetalGridView else { return panes.first }
        return panes.first { $0.view === responder }
    }

    func focus(_ pane: PaneController) {
        window?.makeFirstResponder(pane.view)
        onFocusChange?(pane)
    }

    /// Replaces `pane`'s view in the tree with a split holding it and
    /// `newPane`, then focuses the new one.
    func split(_ pane: PaneController, with newPane: PaneController, vertical: Bool) {
        if zoomed != nil {
            toggleZoom(pane)
        }
        let old = pane.view
        let container = old.superview
        let split = NSSplitView()
        split.isVertical = vertical
        split.dividerStyle = .thin
        split.autoresizingMask = [.width, .height]
        split.frame = old.frame
        if let parent = container as? NSSplitView {
            let index = parent.arrangedSubviews.firstIndex(of: old) ?? 0
            parent.removeArrangedSubview(old)
            old.removeFromSuperview()
            parent.insertArrangedSubview(split, at: index)
        } else {
            install(split)
        }
        split.addArrangedSubview(old)
        split.addArrangedSubview(newPane.view)
        split.adjustSubviews()
        split.setPosition(vertical ? split.bounds.width / 2 : split.bounds.height / 2, ofDividerAt: 0)
        panes.append(newPane)
        focus(newPane)
    }

    /// Removes `pane`; its sibling takes the space. Returns false when it
    /// was the last pane (the caller closes the window instead).
    func remove(_ pane: PaneController) -> Bool {
        guard panes.count > 1, let index = panes.firstIndex(where: { $0 === pane }) else { return false }
        if zoomed != nil {
            toggleZoom(pane)
        }
        let view = pane.view
        guard let split = view.superview as? NSSplitView else { return false }
        split.removeArrangedSubview(view)
        view.removeFromSuperview()
        let sibling = split.arrangedSubviews[0]
        split.removeArrangedSubview(sibling)
        sibling.removeFromSuperview()
        if let parent = split.superview as? NSSplitView {
            let i = parent.arrangedSubviews.firstIndex(of: split) ?? 0
            parent.removeArrangedSubview(split)
            split.removeFromSuperview()
            parent.insertArrangedSubview(sibling, at: i)
            parent.adjustSubviews()
        } else {
            install(sibling)
        }
        panes.remove(at: index)
        pane.detach()
        focus(panes[min(index, panes.count - 1)])
        return true
    }

    func toggleZoom(_ pane: PaneController) {
        if let zoomed {
            for p in panes where p !== zoomed {
                p.view.isHidden = false
            }
            self.zoomed = nil
            root?.needsLayout = true
        } else {
            for p in panes where p !== pane {
                p.view.isHidden = true
            }
            zoomed = pane
        }
        focus(pane)
    }

    /// The nearest pane whose edge lies in `direction` from `pane`.
    func neighbour(of pane: PaneController, direction: ShellAction.Direction) -> PaneController? {
        let from = pane.view.convert(pane.view.bounds, to: self)
        let centre = CGPoint(x: from.midX, y: from.midY)
        var best: (PaneController, CGFloat)?
        for candidate in panes where candidate !== pane && !candidate.view.isHidden {
            let r = candidate.view.convert(candidate.view.bounds, to: self)
            let distance: CGFloat
            switch direction {
            case .left where r.maxX <= from.minX + 1: distance = from.minX - r.maxX
            case .right where r.minX >= from.maxX - 1: distance = r.minX - from.maxX
            case .up where r.maxY <= from.minY + 1: distance = from.minY - r.maxY
            case .down where r.minY >= from.maxY - 1: distance = r.minY - from.maxY
            default: continue
            }
            // Prefer the pane that overlaps the current one's centre line.
            let overlaps = (direction == .left || direction == .right)
                ? (r.minY ... r.maxY).contains(centre.y)
                : (r.minX ... r.maxX).contains(centre.x)
            let score = distance + (overlaps ? 0 : 10000)
            if best == nil || score < best!.1 {
                best = (candidate, score)
            }
        }
        return best?.0
    }
}
