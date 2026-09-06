// Mouse text selection (12 §A15): drag selects a stream, double-click the
// smart word, triple-click the line, ⌥-drag a rectangle, ⇧-click extends.
// Rows are absolute, so the selection survives scrolling; the text comes
// from the daemon's `session.text`, which knows the scrollback. ⌘C copies
// it; `[terminal] copy_on_select` copies on mouse-up.

import AppKit
import CVambiantTerm

extension MetalGridView {
    /// Column under `point`, clamped to the grid.
    func gridCol(at point: CGPoint) -> Int {
        let scale = window?.backingScaleFactor ?? 2
        let width = renderer.cellSize.width / scale
        guard width > 0, cols > 0 else { return 0 }
        return max(0, min(Int(cols) - 1, Int((point.x - padding.width) / width)))
    }

    /// The grid point under `point`, with the row clamped into the viewport.
    func gridPoint(at point: CGPoint) -> GridPoint {
        let scale = window?.backingScaleFactor ?? 2
        let rowHeight = renderer.cellSize.height / scale
        let raw = rowHeight > 0 ? Int((point.y - padding.height) / rowHeight) : 0
        let row = max(0, min(Int(rows) - 1, raw))
        return GridPoint(row: viewportTop + UInt64(row), col: gridCol(at: point))
    }

    /// Starts or extends a selection from a click that was not on block chrome.
    func beginTextSelection(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let at = gridPoint(at: point)
        let mods = event.modifierFlags
        if mods.contains(.shift), var current = textSelection {
            current.head = at
            textSelection = current
            return
        }
        switch event.clickCount {
        case 2:
            let line = viewer?.withGrid { $0.line(Int(at.row - viewportTop)) } ?? ""
            if let r = TextSelection.smartRange(in: line, at: at.col) {
                textSelection = TextSelection(
                    anchor: GridPoint(row: at.row, col: r.lowerBound),
                    head: GridPoint(row: at.row, col: r.upperBound - 1)
                )
            } else {
                textSelection = nil
            }
        case 3:
            textSelection = TextSelection(anchor: at, head: at, mode: .line)
        default:
            textSelection = TextSelection(anchor: at, head: at, mode: mods.contains(.option) ? .rectangle : .stream)
        }
    }

    override func mouseDragged(with event: NSEvent) {
        guard var current = textSelection else { return }
        let point = convert(event.locationInWindow, from: nil)
        current.head = gridPoint(at: point)
        if current.mode == .line {
            current.head = GridPoint(row: current.head.row, col: current.head.col)
        }
        textSelection = current
    }

    override func mouseUp(with event: NSEvent) {
        guard let current = textSelection else { return }
        if current.isEmpty {
            textSelection = nil
        } else if copyOnSelect {
            copyTextSelection()
        }
    }

    /// The selection's text from the daemon; nil when nothing is selected.
    func selectedText() -> String? {
        guard let sel = textSelection, !sel.isEmpty else { return nil }
        let rows = textProvider?(sel.rows) ?? []
        return sel.text(lines: rows)
    }

    /// ⌘C on a text selection.
    @discardableResult
    func copyTextSelection() -> Bool {
        guard let text = selectedText(), !text.isEmpty else { return false }
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
        return true
    }

    func clearTextSelection() {
        if textSelection != nil {
            textSelection = nil
        }
    }
}

extension VtGridView {
    /// One viewport row as text, trailing blanks kept (callers trim).
    func line(_ r: Int) -> String {
        guard let cells, cols > 0, r >= 0, r < Int(rows) else { return "" }
        let w = Int(cols)
        var line = ""
        for c in 0 ..< w {
            let cell = cells[r * w + c]
            if cell.attrs & UInt16(Attrs.wideSpacer) != 0 {
                continue
            }
            line.unicodeScalars.append(Unicode.Scalar(cell.ch) ?? " ")
        }
        return line
    }
}
