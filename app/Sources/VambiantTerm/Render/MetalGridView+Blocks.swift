// Block interaction on the pane: wheel scrolling through scrollback,
// clicking a block's gutter or header to select it, the kebab menu
// (docs/06 §4) on right-click, and the keyboard scroll steps.

import AppKit
import CVambiantTerm

extension MetalGridView {
    /// Grid row under a point in view coordinates, if inside the grid.
    func gridRow(at point: CGPoint) -> Int? {
        let scale = window?.backingScaleFactor ?? 2
        let rowHeight = renderer.cellSize.height / scale
        let y = point.y - padding.height
        guard rowHeight > 0, y >= 0 else { return nil }
        let row = Int(y / rowHeight)
        return row < Int(rows) ? row : nil
    }

    /// The command block drawn on the visible row under `point`.
    func block(at point: CGPoint) -> Block? {
        guard let row = gridRow(at: point) else { return nil }
        return blocks.command(at: viewportTop + UInt64(row))
    }

    override func scrollWheel(with event: NSEvent) {
        guard let viewer else { return }
        let scale = window?.backingScaleFactor ?? 2
        let rowHeight = renderer.cellSize.height / scale
        guard rowHeight > 0 else { return }
        // Precise (trackpad) deltas are points; wheel clicks are rows.
        let dy = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * rowHeight
        scrollRemainder += dy
        let rowsMoved = Int(scrollRemainder / rowHeight)
        guard rowsMoved != 0 else { return }
        scrollRemainder -= CGFloat(rowsMoved) * rowHeight
        // Content moving down (positive delta) reveals older rows.
        viewer.scroll(VtScrollTo_Lines, n: Int64(-rowsMoved))
    }

    func scroll(_ step: ShellAction.ScrollStep) {
        guard let viewer else { return }
        let page = Int64(max(1, Int(rows) - 1))
        switch step {
        case .pageUp: viewer.scroll(VtScrollTo_Lines, n: -page)
        case .pageDown: viewer.scroll(VtScrollTo_Lines, n: page)
        case .top: viewer.scroll(VtScrollTo_Top)
        case .bottom: viewer.scroll(VtScrollTo_Bottom)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        let inGutter = point.x < padding.width
        guard let block = block(at: point) else {
            selectedBlock = nil
            return
        }
        // Clicking the gutter or the header row selects the whole block;
        // clicking inside its output only deselects (future text selection).
        let headerRow = block.start >= viewportTop ? Int(block.start - viewportTop) : -1
        if inGutter || gridRow(at: point) == headerRow {
            selectedBlock = selectedBlock == block.seq ? nil : block.seq
        } else {
            selectedBlock = nil
        }
    }

    override func rightMouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard let block = block(at: point) else {
            super.rightMouseDown(with: event)
            return
        }
        selectedBlock = block.seq
        let menu = NSMenu(title: "Block")
        let items: [(String, Selector, Bool)] = [
            ("Copy Command", #selector(copyCommand(_:)), block.cmdline != nil),
            ("Copy Output", #selector(copyOutput(_:)), block.outputRows != nil),
            ("Re-run Command", #selector(rerunCommand(_:)), block.cmdline != nil),
            ("Explain (needs the provider layer, M-AI)", #selector(explainBlock(_:)), false),
        ]
        for (title, selector, enabled) in items {
            let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
            item.target = self
            item.isEnabled = enabled
            menu.addItem(item)
        }
        menu.autoenablesItems = false
        NSMenu.popUpContextMenu(menu, with: event, for: self)
    }

    private func act(_ action: BlockAction) {
        guard let block = selectedBlock.flatMap(blocks.command(seq:)) else {
            NSSound.beep()
            return
        }
        onBlockAction?(action, block)
    }

    @objc func copyCommand(_ sender: Any?) {
        act(.copyCommand)
    }

    @objc func copyOutput(_ sender: Any?) {
        act(.copyOutput)
    }

    @objc func rerunCommand(_ sender: Any?) {
        act(.rerun)
    }

    @objc func explainBlock(_ sender: Any?) {
        act(.explain)
    }

    @objc func previousPrompt(_ sender: Any?) {
        onAction?(.promptPrevious)
    }

    @objc func nextPrompt(_ sender: Any?) {
        onAction?(.promptNext)
    }

    @objc func selectPreviousBlock(_ sender: Any?) {
        onAction?(.blockSelectPrevious)
    }

    @objc func selectNextBlock(_ sender: Any?) {
        onAction?(.blockSelectNext)
    }
}
