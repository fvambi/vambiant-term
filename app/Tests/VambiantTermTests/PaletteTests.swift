import Foundation
import Testing
@testable import VambiantTerm

struct PaletteTests {
    private var items: [PaletteItem] {
        [
            PaletteItem(kind: .action(id: "pane.split_right"), title: "Split right", detail: "cmd+d"),
            PaletteItem(kind: .action(id: "sidebar.toggle"), title: "Show or hide the sidebar", detail: "cmd+\\"),
            PaletteItem(kind: .session(paneID: ObjectIdentifier(NSNumber(value: 1))), title: "cargo test", detail: "~/code/app"),
            PaletteItem(kind: .history(command: "git status"), title: "git status", detail: "history"),
            PaletteItem(kind: .file(path: "src/main.rs"), title: "src/main.rs", detail: "file"),
        ]
    }

    @Test func scopesParseWarpsPrefixes() {
        #expect(PaletteScope.parse("sessions: cargo") == (.sessions, "cargo"))
        #expect(PaletteScope.parse("h:git") == (.history, "git"))
        #expect(PaletteScope.parse("  files:  ") == (.files, ""))
        #expect(PaletteScope.parse("split") == (.all, "split"))
    }

    @Test func rankingPrefersWordStartsAndRuns() throws {
        #expect(PaletteRanker.score("sr", in: "Split right") != nil)
        #expect(PaletteRanker.score("xyz", in: "Split right") == nil)
        let s1 = try #require(PaletteRanker.score("main", in: "src/main.rs"))
        let s2 = try #require(PaletteRanker.score("main", in: "domain/remaining.txt"))
        #expect(s1 > s2, "a word-start run beats scattered letters")
        let ranked = PaletteRanker.rank(items, scope: .all, text: "git")
        #expect(ranked.first?.title == "git status")
        #expect(PaletteRanker.rank(items, scope: .actions, text: "").count == 2)
        #expect(PaletteRanker.rank(items, scope: .files, text: "main").map(\.title) == ["src/main.rs"])
        #expect(PaletteRanker.rank(items, scope: .sessions, text: "app").first?.title == "cargo test", "details match too")
    }

    @Test func paletteChordResolves() {
        var k = Keymap()
        #expect(k.resolve(KeyChord("p", command: true, shift: true)) == .action(.paletteOpen))
    }
}
