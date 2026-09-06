import Foundation
import Testing
@testable import VambiantTerm

struct InboxTests {
    private let bash = """
    {"id":"toolu_9","session":"abc","session_name":"fake-claude",
     "request":{"id":"toolu_9","tool":"Bash","input":{"command":"rm -rf build"},"reason":null,"source":"PermissionRequest"},
     "hook_event":"PermissionRequest","requested_at":"2026-09-06T18:00:00Z","waiting_secs":75,"prompt_shown":false,"reminders":0,
     "verdict":{"class":"destructive","findings":[{"class":"destructive","rule":"rm-recursive","token":"build",
       "detail":"deletes directories and everything under them","outside_worktree":true}],"commands":1},
     "floor":{"reason":"destructive_outside_worktree","token":"build"}}
    """

    @Test func decodesTheDaemonsItemAndSummarises() throws {
        let item = try JSONDecoder().decode(InboxItem.self, from: Data(bash.utf8))
        #expect(item.command == "rm -rf build")
        #expect(item.summary == "rm -rf build")
        #expect(item.waitingLabel == "1m 15s")
        #expect(item
            .verdictLine == "⚠ destructive — rm-recursive: `build` — deletes directories and everything under them (outside the worktree)")
        #expect(item.floorLine == "never auto-approved: destructive outside the worktree")
        #expect(item.askedBecause == "autonomy is off: every Bash request is asked")
    }

    @Test func editRequestsSummariseAsFileAndCounts() throws {
        let edit = """
        {"id":"t1","session":"s","session_name":"codex","request":{"id":"t1","tool":"Edit",
         "input":{"file_path":"src/api/client.ts","old_string":"a\\nb","new_string":"a\\nb\\nc\\nd"},"source":"PreToolUse"},
         "hook_event":"PreToolUse","requested_at":"2026-09-06T18:00:01Z","waiting_secs":3,"prompt_shown":true,"reminders":0}
        """
        let item = try JSONDecoder().decode(InboxItem.self, from: Data(edit.utf8))
        #expect(item.command == nil)
        #expect(item.summary == "Edit src/api/client.ts (+4 −2)")
        #expect(item.verdictLine == nil && item.floorLine == nil)
        #expect(item.askedBecause.contains("own prompt"))
    }

    @Test func decisionsEncodeAsTheDaemonExpects() throws {
        let allow = try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(InboxDecision.allow(updatedCommand: nil)))
        #expect(allow[path: "behavior"]?.stringValue == "allow")
        let edited = try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(InboxDecision.allow(updatedCommand: "ls")))
        #expect(edited[path: "updated_input.command"]?.stringValue == "ls")
        let deny = try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(InboxDecision.deny(reason: "no")))
        #expect(deny[path: "behavior"]?.stringValue == "deny" && deny[path: "reason"]?.stringValue == "no")
    }

    @Test func listSortsOldestFirstAndFiltersBySession() throws {
        let a = try JSONDecoder().decode(InboxItem.self, from: Data(bash.utf8))
        let newer = try JSONDecoder().decode(
            InboxItem.self, from: Data(bash.replacingOccurrences(of: "18:00:00Z", with: "18:05:00Z").replacingOccurrences(
                of: "\"abc\"",
                with: "\"other\""
            ).utf8)
        )
        var list = InboxList()
        list.replace(with: [newer, a])
        #expect(list.items.map(\.session) == ["abc", "other"])
        #expect(list.pending(for: "other").count == 1)
        #expect(list.count == 2)
        #expect(ShellAction.from(id: "inbox.open", label: "", milestone: "") == .inboxOpen)
    }
}
