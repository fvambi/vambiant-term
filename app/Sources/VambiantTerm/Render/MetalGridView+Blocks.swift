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
            beginTextSelection(with: event)
            return
        }
        // Clicking the gutter or the header row selects the whole block;
        // anywhere else starts a text selection.
        let headerRow = block.start >= viewportTop ? Int(block.start - viewportTop) : -1
        guard inGutter || gridRow(at: point) == headerRow else {
            selectedBlock = nil
            beginTextSelection(with: event)
            return
        }
        clearTextSelection()
        let mods = event.modifierFlags
        if mods.contains(.shift), let anchor = selectionAnchor {
            // ⇧-click: the range from the anchor, like Warp and Finder.
            selectedBlocks = blocks.range(from: anchor, to: block.seq)
        } else if mods.contains(.command) {
            // ⌘-click toggles one block in and out of the selection.
            var set = selectedBlocks
            if set.contains(block.seq) {
                set.remove(block.seq)
            } else {
                set.insert(block.seq)
            }
            selectedBlocks = set
            selectionAnchor = block.seq
        } else {
            selectedBlock = selectedBlocks == [block.seq] ? nil : block.seq
        }
    }

    /// ⌘⌥⇧↑/↓: grow the selection by one block at either end.
    func extendSelection(previous: Bool) {
        guard let anchor = selectionAnchor ?? selectedBlocks.first else {
            onAction?(previous ? .blockSelectPrevious : .blockSelectNext)
            return
        }
        let ordered = blocks.ordered(selectedBlocks)
        let edge = previous ? ordered.first?.seq : ordered.last?.seq
        guard let edge, let next = blocks.neighbour(of: edge, previous: previous) else {
            NSSound.beep()
            return
        }
        selectedBlocks = blocks.range(from: anchor, to: next.seq).union(selectedBlocks)
    }

    /// `⌃M`: the same menu right-click shows, anchored at the header row.
    func openBlockMenu() {
        guard let block = selectedBlock.flatMap(blocks.command(seq:)), let window else {
            NSSound.beep()
            return
        }
        let row = block.start >= viewportTop ? Int(block.start - viewportTop) : 0
        let scale = window.backingScaleFactor
        let y = padding.height + (CGFloat(row) + 1) * renderer.cellSize.height / scale
        let menu = blockMenu(for: block)
        menu.popUp(positioning: nil, at: CGPoint(x: padding.width, y: y), in: self)
    }

    override func rightMouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard let block = block(at: point) else {
            super.rightMouseDown(with: event)
            return
        }
        if !selectedBlocks.contains(block.seq) {
            selectedBlock = block.seq
        }
        NSMenu.popUpContextMenu(blockMenu(for: block), with: event, for: self)
    }

    /// docs/06 §4's kebab menu. Chords are not shown: the keymap owns them.
    private func blockMenu(for block: Block) -> NSMenu {
        let menu = NSMenu(title: "Block")
        let several = selectedBlocks.count > 1
        let items: [(String, Selector, Bool)] = [
            (several ? "Copy Commands" : "Copy Command", #selector(copyCommand(_:)), block.cmdline != nil || several),
            (several ? "Copy Outputs" : "Copy Output", #selector(copyOutput(_:)), block.outputRows != nil || several),
            ("Copy Command and Output", #selector(copyBoth(_:)), true),
            ("Copy as HTML", #selector(exportBlock(_:)), true),
            ("Re-input Command", #selector(reinputCommand(_:)), block.cmdline != nil && !several),
            ("Re-input as Root", #selector(reinputSudo(_:)), block.cmdline != nil && !several),
            ("Re-run Command", #selector(rerunCommand(_:)), block.cmdline != nil && !several),
            (block.bookmarked ? "Remove Bookmark" : "Bookmark", #selector(toggleBookmark(_:)), !several),
            ("Explain (needs the provider layer, M-AI)", #selector(explainBlock(_:)), false),
        ]
        for (title, selector, enabled) in items {
            let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
            item.target = self
            item.isEnabled = enabled
            menu.addItem(item)
        }
        menu.autoenablesItems = false
        return menu
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

    @objc func copyBoth(_ sender: Any?) {
        act(.copyBoth)
    }

    @objc func exportBlock(_ sender: Any?) {
        act(.exportHTML)
    }

    @objc func reinputCommand(_ sender: Any?) {
        act(.reinput)
    }

    @objc func reinputSudo(_ sender: Any?) {
        act(.reinputSudo)
    }

    @objc func toggleBookmark(_ sender: Any?) {
        act(.bookmark)
    }

    @objc func clearScrollback(_ sender: Any?) {
        onAction?(.clearScrollback)
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
