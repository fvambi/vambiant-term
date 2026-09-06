// Clickable links (12 §E4): the URL or path under the pointer is
// underlined with a pointing hand; ⌘-click opens a URL in the browser and
// a file in `[editor] program` (else the default app), with `path:line:col`
// forms honoured. Detection runs over the row's text, so it costs nothing
// until the mouse moves.

import AppKit
import CVambiantTerm

enum LinkKind: Equatable, Sendable {
    case url
    case path
}

struct Link: Equatable, Sendable {
    let kind: LinkKind
    let text: String
    /// Viewport row and scalar columns.
    let row: Int
    let range: Range<Int>

    /// `src/main.rs:12:3` → (`src/main.rs`, 12, 3); a trailing `:` is dropped.
    var pathAndPosition: (path: String, line: Int?, col: Int?) {
        var parts = text.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
        var line: Int?
        var col: Int?
        if parts.count >= 3, let l = Int(parts[parts.count - 2]), let c = Int(parts[parts.count - 1]) {
            line = l
            col = c
            parts.removeLast(2)
        } else if parts.count >= 2, let l = Int(parts[parts.count - 1]) {
            line = l
            parts.removeLast()
        }
        var path = parts.joined(separator: ":")
        while path.hasSuffix(":") || path.hasSuffix(".") || path.hasSuffix(",") {
            path.removeLast()
        }
        return (path, line, col)
    }

    /// The URL or path under `col` in `line`, if any.
    static func at(line: String, col: Int, row: Int) -> Link? {
        let scalars = Array(line.unicodeScalars)
        guard col >= 0, col < scalars.count else { return nil }
        let patterns: [(String, LinkKind)] = [
            (#"https?://[^\s'"<>()]+"#, .url),
            (#"(?:~|\.{1,2})?/[^\s'"`:]+(?::\d+(?::\d+)?)?"#, .path),
            (#"\b[\w.\-]+/[\w./\-]+(?::\d+(?::\d+)?)?"#, .path),
            (#"\b[\w\-]+\.(?:rs|swift|ts|tsx|js|py|go|md|toml|json|yml|yaml|c|h|cpp|m|mm|txt)(?::\d+(?::\d+)?)?\b"#, .path),
        ]
        let ns = line as NSString
        for (p, kind) in patterns {
            guard let re = try? NSRegularExpression(pattern: p) else { continue }
            for m in re.matches(in: line, range: NSRange(location: 0, length: ns.length)) {
                guard let r = Range(m.range, in: line) else { continue }
                let lo = line.unicodeScalars.distance(from: line.unicodeScalars.startIndex, to: r.lowerBound)
                let hi = line.unicodeScalars.distance(from: line.unicodeScalars.startIndex, to: r.upperBound)
                if col >= lo, col < hi {
                    var text = String(line[r])
                    while text.hasSuffix(".") || text.hasSuffix(",") || text.hasSuffix(";") || text.hasSuffix(")") {
                        text.removeLast()
                    }
                    return Link(kind: kind, text: text, row: row, range: lo ..< lo + text.unicodeScalars.count)
                }
            }
        }
        return nil
    }
}

extension MetalGridView {
    /// The link under `point`, from the viewport row's text.
    func link(at point: CGPoint) -> Link? {
        guard let row = gridRow(at: point), let viewer else { return nil }
        let col = gridCol(at: point)
        let line = viewer.withGrid { $0.line(row) }
        return Link.at(line: line, col: col, row: row)
    }

    /// Hover: underline the link and show the hand; nothing else changes.
    func updateLinkHover(at point: CGPoint?) {
        let found = point.flatMap(link(at:))
        if found != hoveredLink {
            hoveredLink = found
            lastSeqReset()
        }
    }

    /// ⌘-click on a link. Returns false when there was none.
    func openLink(at point: CGPoint) -> Bool {
        guard let link = link(at: point) else { return false }
        onOpenLink?(link)
        return true
    }

    override func resetCursorRects() {
        super.resetCursorRects()
        if hoveredLink != nil {
            addCursorRect(bounds, cursor: .pointingHand)
        }
    }
}
