// The overlays that sit on top of the grid: the find bar and the sticky
// command header. Both are plain AppKit views; the grid underneath keeps
// rendering, and match highlights go through the renderer like any other
// block chrome.

import AppKit
import CVambiantTerm

extension MetalGridView {
    func installOverlays() {
        addSubview(findBar)
        addSubview(stickyHeader)
        addSubview(actionsBar)
        actionsBar.onAction = { [weak self] action in
            guard let self, let seq = hoverBlock,
                  let block = blocks.command(seq: seq) ?? blocks.chrome.first(where: { $0.seq == seq })
            else { return }
            selectedBlock = block.seq
            if action == .menu {
                openBlockMenu()
            } else {
                onBlockAction?(action, block)
            }
        }
        findBar.isHidden = true
        findBar.onStep = { [weak self] forward in self?.onFindStep?(forward) }
        findBar.onClose = { [weak self] in self?.hideFind() }
        findBar.onChange = { [weak self] query, regex, cs, inBlock in
            guard let self else { return }
            findState.query = query
            findState.regex = regex
            findState.caseSensitive = cs
            findState.inSelectedBlock = inBlock
            onFindChange?(findState)
        }
        stickyHeader.onClick = { [weak self] in
            guard let self, let seq = stickyHeader.seq, let block = blocks.command(seq: seq) else { return }
            viewer?.scroll(VtScrollTo_Row, n: Int64(block.start))
        }
    }

    /// The hover toolbar follows the block under `point` (nil hides it),
    /// anchored to the block's first visible row at the right edge, left of
    /// the exit chip.
    func updateHover(at point: CGPoint?) {
        guard let point, let block = point.x < bounds.width ? block(at: point) : nil else {
            if hoverBlock != nil {
                hoverBlock = nil
                actionsBar.isHidden = true
            }
            return
        }
        let scale = window?.backingScaleFactor ?? 2
        let rowHeight = renderer.cellSize.height / scale
        let top = viewportTop
        let firstVisible = max(block.visualRows.lowerBound, top)
        let y = padding.height + CGFloat(firstVisible - top) * rowHeight
        let chipWidth = CGFloat(BlockDecor.chip(for: block).count + 2) * renderer.cellSize.width / scale
        actionsBar.frame = CGRect(
            x: bounds.width - padding.width - chipWidth - BlockActionsBar.size.width - 6,
            y: y + (rowHeight - BlockActionsBar.size.height) / 2,
            width: BlockActionsBar.size.width, height: BlockActionsBar.size.height
        )
        if hoverBlock != block.seq {
            hoverBlock = block.seq
            actionsBar.apply(theme: renderer.theme)
            actionsBar.show(for: block)
        }
    }

    func layoutOverlays() {
        var y: CGFloat = 0
        if !findBar.isHidden {
            findBar.frame = CGRect(x: 0, y: 0, width: bounds.width, height: FindBar.height)
            y = FindBar.height
        }
        let scale = window?.backingScaleFactor ?? 2
        let rowHeight = renderer.cellSize.height / scale
        stickyHeader.frame = CGRect(x: 0, y: y, width: bounds.width, height: rowHeight + 4)
    }

    /// Called per frame with the viewport's top row: the header shows the
    /// block whose command line is above the fold, if any.
    func updateStickyHeader(top: UInt64) {
        guard stickyHeaderEnabled, renderer.blockChrome.stickyHeader,
              let block = blocks.command(at: top), block.start < top
        else {
            if !stickyHeader.isHidden {
                stickyHeader.hide()
            }
            return
        }
        if stickyHeader.seq != block.seq || stickyHeader.isHidden {
            stickyHeader.show(block: block, font: renderer.nsFont, theme: renderer.theme)
        }
    }

    func showFind() {
        findBar.isHidden = false
        needsLayout = true
        findBar.focus()
    }

    func hideFind() {
        findBar.isHidden = true
        findState = FindState()
        findBar.state = findState
        needsLayout = true
        lastSeqReset()
        window?.makeFirstResponder(self)
    }

    /// Results from the pane; the bar shows the count and the grid the marks.
    func applyFind(_ state: FindState) {
        findState = state
        findBar.state = state
        lastSeqReset()
    }

    @objc func findAction(_ sender: Any?) {
        onAction?(.findOpen)
    }

    @objc func findNextAction(_ sender: Any?) {
        onAction?(.findNext)
    }

    @objc func findPreviousAction(_ sender: Any?) {
        onAction?(.findPrevious)
    }

    @objc func toggleStickyHeader(_ sender: Any?) {
        stickyHeaderEnabled.toggle()
        lastSeqReset()
    }
}
