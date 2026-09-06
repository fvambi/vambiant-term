import Foundation
import Testing
@testable import VambiantTerm

struct MailboxTests {
    private func note(_ id: String, _ kind: NoteKind, session: String = "s", at: Date) -> Note {
        Note(id: id, kind: kind, title: "t \(id)", body: "b \(id)", session: session, at: at)
    }

    @Test func coalescesSameSessionAndKindInsideTheWindow() {
        var m = Mailbox()
        m.coalesceWindow = 10
        let t0 = Date(timeIntervalSince1970: 1000)
        m.add(note("a", .request, at: t0))
        let merged = m.add(note("b", .request, at: t0.addingTimeInterval(3)))
        #expect(merged.count == 2 && merged.title == "2 approvals waiting" && merged.body == "b b")
        #expect(m.notes.count == 1)
        m.add(note("c", .request, session: "other", at: t0.addingTimeInterval(4)))
        m.add(note("d", .request, at: t0.addingTimeInterval(30)))
        #expect(m.notes.count == 3, "another session and a note outside the window stay separate")
        #expect(m.unread == 3)
        m.markRead(id: "d")
        #expect(m.unread == 2 && m.filtered(.unread).count == 2)
        m.add(note("e", .error, at: t0.addingTimeInterval(31)))
        #expect(m.filtered(.errors).map(\.id) == ["e"])
        m.markAllRead()
        #expect(m.unread == 0)
    }

    @Test func policyFollowsDocs06Section8() {
        let p = NotificationPolicy()
        #expect(p.route(.complete, source: .longCommand, appActive: true, paneVisible: true) == (false, false), "visible pane: nothing")
        #expect(p.route(.complete, source: .longCommand, appActive: true, paneVisible: false) == (true, false), "other pane: toast")
        #expect(p.route(.complete, source: .longCommand, appActive: false, paneVisible: false) == (false, true), "background: desktop")
        #expect(p.route(.request, source: .approval, appActive: false, paneVisible: false) == (false, false), "the daemon posts approvals")
        #expect(p.route(.request, source: .approval, appActive: true, paneVisible: false) == (true, false))
        #expect(
            p.route(.complete, source: .agentFinished, appActive: true, paneVisible: false) == (false, false),
            "when_unfocused: no toast"
        )
        #expect(p.route(.complete, source: .agentFinished, appActive: false, paneVisible: false) == (false, true))
        var always = p
        always.agentFinished = .always
        #expect(always.route(.complete, source: .agentFinished, appActive: true, paneVisible: false) == (true, false))
        var quiet = p
        quiet.agentCrashed = false
        #expect(quiet.route(.error, source: .agentCrashed, appActive: false, paneVisible: false) == (false, false))
        #expect(p.route(.error, source: .agentCrashed, appActive: true, paneVisible: true) == (true, false))
    }

    @Test func longCommandsBecomeNotesAboveTheThreshold() throws {
        let json = """
        {"seq": 7, "block": {"kind": {"kind": "command", "cmdline": "cargo test --workspace", "exit": 0}, "confidence": "marked",
         "start_line": 0, "end_line": 5, "duration_ms": 42000}}
        """
        let block = try #require(try Block.parse(item: JSONDecoder().decode(JSONValue.self, from: Data(json.utf8))))
        let note = try #require(Note.longCommand(block, session: "s", thresholdMs: 30000))
        #expect(note.kind == .complete && note.title == "Command finished · 42.0s" && note.body == "cargo test --workspace")
        #expect(Note.longCommand(block, session: "s", thresholdMs: 60000) == nil)
        let failed = try #require(try Block.parse(item: JSONDecoder().decode(
            JSONValue.self, from: Data(json.replacingOccurrences(of: "\"exit\": 0", with: "\"exit\": 101").replacingOccurrences(
                of: "42000",
                with: "125000"
            ).utf8)
        )))
        let failure = try #require(Note.longCommand(failed, session: "s", thresholdMs: 30000))
        #expect(failure.kind == .error && failure.title == "Command failed (exit 101) · 2m 05s")
        #expect(MailboxSheet.relative(Date(timeIntervalSinceNow: -125), now: Date()) == "2m ago")
        #expect(ShellAction.from(id: "mailbox.open", label: "", milestone: "") == .mailboxOpen)
    }
}
