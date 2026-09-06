import Foundation
import Testing
@testable import VambiantTerm

struct PolicyTests {
    private let destructive = """
    {"verdict":{"class":"destructive","findings":[{"class":"destructive","rule":"rm-recursive","token":"/",
     "detail":"deletes directories and everything under them","outside_worktree":true}],"commands":1},
     "floor":{"reason":"destructive_outside_worktree","token":"/"},"decision":"confirm"}
    """

    @Test func decodesTheDaemonsVerdict() throws {
        let v = try JSONDecoder().decode(SafetyVerdict.self, from: Data(destructive.utf8))
        #expect(v.needsConfirm && !v.isBlocked && !v.isBenign)
        #expect(v.label == "destructive · never auto")
        #expect(v.explanation == "rm-recursive: `/` — deletes directories and everything under them (outside the worktree)")
    }

    @Test func benignAndBlockedShapes() throws {
        let benign = try JSONDecoder().decode(
            SafetyVerdict.self,
            from: Data("{\"verdict\":{\"class\":\"benign\",\"findings\":[],\"commands\":2},\"decision\":\"allow\"}".utf8)
        )
        #expect(benign.isBenign && benign.label == "benign" && benign.explanation.isEmpty)
        let unparseable = """
        {"verdict":{"class":"unparseable","findings":[],"parse_error":"unterminated ' quote","commands":0},
         "floor":{"reason":"parse_failed","detail":"x"},"decision":"block"}
        """
        let blocked = try JSONDecoder().decode(SafetyVerdict.self, from: Data(unparseable.utf8))
        #expect(blocked.isBlocked)
        #expect(blocked.explanation == "could not be parsed: unterminated ' quote")
    }
}
