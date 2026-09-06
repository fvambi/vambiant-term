import Foundation
import Testing
@testable import VambiantTerm

struct SelectionTests {
    private func p(_ row: UInt64, _ col: Int) -> GridPoint {
        GridPoint(row: row, col: col)
    }

    @Test func streamSelectionSlicesFirstAndLastRows() {
        let sel = TextSelection(anchor: p(11, 4), head: p(10, 2))
        #expect(sel.start == p(10, 2) && sel.end == p(11, 4), "normalised whichever way it was dragged")
        #expect(sel.text(lines: ["hello world  ", "second line"]) == "llo world\nsecon")
        let spans = sel.spans(top: 9, rows: 4, cols: 8)
        #expect(spans == [SelectionSpan(row: 1, col: 2, len: 6), SelectionSpan(row: 2, col: 0, len: 5)])
        #expect(TextSelection(anchor: p(1, 1), head: p(1, 1)).isEmpty)
        #expect(TextSelection(anchor: p(1, 1), head: p(1, 1)).spans(top: 0, rows: 3, cols: 8).isEmpty)
    }

    @Test func lineAndRectangleModes() {
        let line = TextSelection(anchor: p(3, 5), head: p(4, 0), mode: .line)
        #expect(line.text(lines: ["abc   ", "def"]) == "abc\ndef")
        #expect(line.spans(top: 3, rows: 2, cols: 10) == [SelectionSpan(row: 0, col: 0, len: 10), SelectionSpan(row: 1, col: 0, len: 10)])
        let rect = TextSelection(anchor: p(0, 4), head: p(2, 1), mode: .rectangle)
        #expect(rect.text(lines: ["0123456", "abcdefg", "xy"]) == "1234\nbcde\ny")
        #expect(rect.spans(top: 0, rows: 3, cols: 7).allSatisfy { $0.col == 1 && $0.len == 4 })
    }

    @Test func smartSelectFindsUrlsPathsAndWords() {
        let line = "see https://example.com/a?b=1 or ~/code/app/src/main.rs at 10.0.0.9:8080 v1.25"
        func pick(_ col: Int) -> String? {
            TextSelection.smartRange(in: line, at: col).map { String(String.UnicodeScalarView(Array(line.unicodeScalars)[$0])) }
        }
        #expect(pick(8) == "https://example.com/a?b=1")
        #expect(pick(36) == "~/code/app/src/main.rs")
        #expect(pick(60) == "10.0.0.9:8080")
        #expect(pick(1) == "see")
        #expect(TextSelection.smartRange(in: "a  b", at: 1) == nil, "a blank selects nothing")
    }
}
