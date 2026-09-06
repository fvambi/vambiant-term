import Foundation
import Testing
@testable import VambiantTerm

struct CommandHighlighterTests {
    private func kinds(_ text: String) -> [(String, ShellToken)] {
        let s = Array(text.unicodeScalars)
        return CommandHighlighter.spans(text).map { (String(String.UnicodeScalarView(s[$0.range])), $0.kind) }
    }

    @Test func classifiesAPipeline() {
        let k = kinds("git log --oneline -n 5 | grep \"fix\" > out.txt # notes")
        #expect(k.map(\.1) == [.command, .arg, .flag, .flag, .arg, .op, .command, .string, .op, .arg, .comment])
        #expect(k[6].0 == "grep")
        #expect(k[7].0 == "\"fix\"")
    }

    @Test func assignmentsVariablesAndSeparators() {
        let k = kinds("FOO=1 echo $FOO; ls && pwd")
        #expect(k.map(\.1) == [.variable, .command, .variable, .op, .command, .op, .command])
        #expect(kinds("'unterminated").map(\.1) == [.string])
        #expect(kinds("").isEmpty)
    }
}

struct AutosuggestTests {
    @Test func suggestsTheNewestMatchingEntry() {
        let h = ["git status", "git stash", "ls -la"]
        #expect(Autosuggest.ghost(for: "git st", history: h) == "atus")
        #expect(Autosuggest.ghost(for: "git status", history: h) == nil, "nothing left to suggest")
        #expect(Autosuggest.ghost(for: "", history: h) == nil)
        #expect(Autosuggest.ghost(for: "x", history: h) == nil)
        #expect(Autosuggest.ghost(for: "a\nb", history: ["a\nbc"]) == nil, "multi-line input gets no ghost")
    }

    @Test func acceptsOneWordAtATime() {
        #expect(Autosuggest.firstWord(of: " -la --color") == " -la ")
        #expect(Autosuggest.firstWord(of: "atus") == "atus")
        #expect(Autosuggest.firstWord(of: "") == "")
    }
}
