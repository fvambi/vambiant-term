import Foundation
import Testing
@testable import VambiantTerm

struct GitProbeTests {
    @Test func headParsesBranchesAndDetachedHeads() {
        #expect(GitProbe.branch(fromHead: "ref: refs/heads/main\n") == "main")
        #expect(GitProbe.branch(fromHead: "ref: refs/heads/feature/x") == "feature/x")
        #expect(GitProbe.branch(fromHead: "ref: refs/tags/v1") == "refs/tags/v1")
        #expect(GitProbe.branch(fromHead: "0123456789abcdef\n") == "01234567")
        #expect(GitProbe.branch(fromHead: "") == nil)
    }

    @Test func walksUpAndFollowsWorktreeFiles() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("vt-git-\(UUID().uuidString)")
        let repo = root.appendingPathComponent("repo")
        let nested = repo.appendingPathComponent("a/b")
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: repo.appendingPathComponent(".git"), withIntermediateDirectories: true)
        try "ref: refs/heads/topic\n".write(to: repo.appendingPathComponent(".git/HEAD"), atomically: true, encoding: .utf8)
        #expect(GitProbe.branch(for: nested.path) == "topic")
        // A worktree: `.git` is a file pointing at the real gitdir.
        let tree = root.appendingPathComponent("tree")
        let gitdir = repo.appendingPathComponent(".git/worktrees/tree")
        try FileManager.default.createDirectory(at: tree, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: gitdir, withIntermediateDirectories: true)
        try "gitdir: \(gitdir.path)\n".write(to: tree.appendingPathComponent(".git"), atomically: true, encoding: .utf8)
        try "ref: refs/heads/wt\n".write(to: gitdir.appendingPathComponent("HEAD"), atomically: true, encoding: .utf8)
        #expect(GitProbe.branch(for: tree.path) == "wt")
        #expect(GitProbe.branch(for: root.path) == nil)
        try? FileManager.default.removeItem(at: root)
    }

    @Test func abbreviatesHome() {
        let home = NSHomeDirectory()
        #expect(GitProbe.abbreviated(home) == "~")
        #expect(GitProbe.abbreviated(home + "/code") == "~/code")
        #expect(GitProbe.abbreviated("/tmp") == "/tmp")
    }
}

struct BlockHeaderTests {
    private func cmd(_ seq: Int64, _ start: UInt64, _ end: UInt64, ms: UInt64? = nil, cwd: String? = nil) -> Block {
        var b = Block(seq: seq, kind: .command(cmdline: "c", exit: 0), confidence: "marked", start: start, end: end)
        b.durationMs = ms
        b.cwd = cwd
        return b
    }

    @Test func durationsReadLikeWarps() {
        #expect(BlockList.durationText(ms: 27) == "(0.027s)")
        #expect(BlockList.durationText(ms: 1234) == "(1.2s)")
        #expect(BlockList.durationText(ms: 125_000) == "(2m 05s)")
    }

    @Test func headerJoinsWhatIsKnown() {
        let b = cmd(1, 5, 9, ms: 27, cwd: "/tmp/x")
        #expect(BlockDecor.header(for: b, cwd: "/other", branch: "main") == "/tmp/x  git:(main)  (0.027s)")
        #expect(BlockDecor.header(for: cmd(2, 5, 9), cwd: nil, branch: nil) == "")
        #expect(BlockDecor.header(for: cmd(3, 5, 9), cwd: "/tmp", branch: nil) == "/tmp")
    }

    @Test func headerOnlyWhenThePromptRowIsVisible() {
        var l = BlockList()
        l.replace(with: [cmd(1, 5, 9, ms: 10), cmd(2, 9, 12)])
        let headers: (Block) -> String = { "hdr \($0.seq)" }
        let d = BlockDecor.decorations(for: l, top: 4, rows: 10, selected: [], headers: headers)
        #expect(d[0].header == "hdr 1", "row 4 (the prompt row) is on screen")
        #expect(d[1].header == "hdr 2")
        let cut = BlockDecor.decorations(for: l, top: 5, rows: 10, selected: [], headers: headers)
        #expect(cut[0].header == nil, "prompt row scrolled off: no header")
        let classic = BlockDecor.decorations(for: l, top: 4, rows: 10, selected: [], headers: nil)
        #expect(classic.allSatisfy { $0.header == nil })
    }
}

struct SidebarModelTests {
    private func session(_ n: Int, cwd: String?, cmd: String? = nil, state: String? = nil) -> SidebarSession {
        SidebarSession(
            id: ObjectIdentifier(NSNumber(value: n)), name: "s\(n)", cwd: cwd, branch: nil, lastCommand: cmd,
            state: state, agent: nil, diff: nil, focused: false
        )
    }

    @Test func groupsByRepoWithScratchLast() {
        let s = [
            session(1, cwd: "/tmp/other"),
            session(2, cwd: "/r/app/src", cmd: "cargo test"),
            session(3, cwd: "/r/app"),
            session(4, cwd: "/r/web"),
        ]
        let rows = SidebarModel.rows(for: s) { cwd in
            cwd.hasPrefix("/r/app") ? "/r/app" : cwd.hasPrefix("/r/web") ? "/r/web" : nil
        }
        #expect(rows.count == 7)
        #expect(rows[0] == .repo(name: "app", path: "/r/app"))
        #expect(rows[3] == .repo(name: "web", path: "/r/web"))
        #expect(rows[5] == .repo(name: "scratch", path: nil))
        if case let .session(x) = rows[1] {
            #expect(x.title == "cargo test", "the last command is the title")
        }
    }

    @Test func filterKeepsMatchingSessionsWithTheirRepo() {
        let sessions = [session(1, cwd: "/r/app", cmd: "npm test"), session(2, cwd: "/r/app", cmd: "vim")]
        let rows = SidebarModel.rows(for: sessions) { _ in "/r/app" }
        let f = SidebarModel.filter(rows, query: "NPM")
        #expect(f.count == 2)
        #expect(SidebarModel.filter(rows, query: "zzz").isEmpty)
        #expect(SidebarModel.filter(rows, query: "  ") == rows)
    }

    @Test func glyphsAndDiffText() {
        var s = session(1, cwd: nil, state: "awaiting_input")
        #expect(s.stateGlyph == "✋" && s.stateLabel == "waiting for you")
        #expect(session(2, cwd: nil, state: "tool_running").stateGlyph == "⚙")
        #expect(session(3, cwd: nil).stateGlyph == "○")
        s = SidebarSession(
            id: s.id, name: s.name, cwd: nil, branch: nil, lastCommand: nil, state: nil, agent: nil, diff: (34, 9), focused: false
        )
        #expect(s.diffText == "+34 -9")
        #expect(GitProbe.diffStats(numstat: "3\t1\ta.rs\n-\t-\tbin.png\n10\t2\tb.rs\n") == (13, 3))
        #expect(GitProbe.diffStats(numstat: "") == (0, 0))
    }
}
