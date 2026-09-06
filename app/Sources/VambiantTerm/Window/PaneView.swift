// One pane: the grid on top, Warp's input area at the bottom (ADR-0011).
// In classic mode the input area is hidden and the grid fills the pane.

import AppKit

@MainActor
final class PaneView: NSView {
    let grid: MetalGridView
    let input = InputAreaView()
    var warpMode = true {
        didSet {
            input.isHidden = !warpMode
            needsLayout = true
        }
    }

    init(grid: MetalGridView) {
        self.grid = grid
        super.init(frame: .zero)
        addSubview(grid)
        addSubview(input)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    override var isFlipped: Bool {
        true
    }

    override func layout() {
        super.layout()
        let scale = window?.backingScaleFactor ?? 2
        let lineHeight = grid.renderer.cellSize.height / scale
        let inputHeight = warpMode ? InputAreaView.height(lines: input.editor.lineCount, lineHeight: lineHeight) : 0
        input.relayout(lineHeight: lineHeight)
        input.frame = CGRect(x: 0, y: bounds.height - inputHeight, width: bounds.width, height: inputHeight)
        grid.frame = CGRect(x: 0, y: 0, width: bounds.width, height: bounds.height - inputHeight)
    }
}
