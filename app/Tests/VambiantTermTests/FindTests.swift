import Foundation
import Testing
@testable import VambiantTerm

struct FindStateTests {
    private func m(_ row: UInt64, _ col: Int = 0) -> FindMatch {
        FindMatch(row: row, col: col, len: 3)
    }

    @Test func resultsStartFromTheBottomOfTheViewport() {
        var s = FindState(query: "x")
        s.replace(with: [m(2), m(10), m(40)], viewportBottom: 12)
        #expect(s.current == 1, "last match at or above the viewport bottom")
        #expect(s.summary == "2 of 3")
        s.replace(with: [m(50)], viewportBottom: 12)
        #expect(s.current == 0, "nothing above: the last match")
        s.replace(with: [], viewportBottom: 12)
        #expect(s.current == nil)
        #expect(s.summary == "no matches")
    }

    @Test func steppingWrapsBothWays() {
        var s = FindState(query: "x")
        s.replace(with: [m(1), m(2), m(3)], viewportBottom: 99)
        #expect(s.current == 2)
        #expect(s.step(forward: true)?.row == 1)
        #expect(s.step(forward: false)?.row == 3)
        #expect(s.step(forward: false)?.row == 2)
        var empty = FindState()
        #expect(empty.step(forward: true) == nil)
    }

    @Test func visibleMatchesMapToGridRows() {
        var s = FindState(query: "x")
        s.replace(with: [m(5, 2), m(7, 0), m(30)], viewportBottom: 99)
        let v = s.visible(top: 5, rows: 4)
        #expect(v == [
            MatchDecoration(row: 0, col: 2, len: 3, current: false),
            MatchDecoration(row: 2, col: 0, len: 3, current: false),
        ])
        #expect(s.visible(top: 5, rows: 0).isEmpty)
    }

    @Test func errorsReplaceTheCount() {
        var s = FindState(query: "(")
        s.error = "bad `query`: unclosed group"
        #expect(s.summary.contains("unclosed"))
        #expect(FindState().summary.isEmpty)
    }

    @Test func chordsResolve() {
        var k = Keymap()
        #expect(k.resolve(KeyChord("f", command: true)) == .action(.findOpen))
        #expect(k.resolve(KeyChord("g", command: true, shift: true)) == .action(.findPrevious))
    }
}
