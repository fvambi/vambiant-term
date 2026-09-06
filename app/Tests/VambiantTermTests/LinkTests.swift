import Foundation
import Testing
@testable import VambiantTerm

struct LinkTests {
    @Test func detectsUrlsAndPathsUnderTheColumn() {
        let line = "error at src/main.rs:12:3 see https://example.com/docs?x=1. or ~/notes.txt and (README.md)"
        func at(_ col: Int) -> Link? {
            Link.at(line: line, col: col, row: 0)
        }
        #expect(at(10)?.text == "src/main.rs:12:3" && at(10)?.kind == .path)
        #expect(at(10)?.pathAndPosition.line == 12 && at(10)?.pathAndPosition.col == 3)
        #expect(at(10)?.pathAndPosition.path == "src/main.rs")
        #expect(at(32)?.text == "https://example.com/docs?x=1", "a trailing full stop is not part of the URL")
        #expect(at(32)?.kind == .url)
        #expect(at(64)?.text == "~/notes.txt")
        #expect(at(82)?.text == "README.md")
        #expect(at(3) == nil, "plain words are not links")
        #expect(at(64)?.range == 63 ..< 74)
    }

    @Test func editorsGetTheirOwnPositionForm() {
        #expect(PaneController.editorArguments("/usr/local/bin/code", path: "/a/b.rs", line: 3, col: 7) == ["-g", "/a/b.rs:3:7"])
        #expect(PaneController.editorArguments("zed", path: "/a/b.rs", line: 3, col: nil) == ["/a/b.rs:3"])
        #expect(PaneController.editorArguments("idea", path: "/a/b.rs", line: 3, col: nil) == ["--line", "3", "/a/b.rs"])
        #expect(PaneController.editorArguments("nvim", path: "/a/b.rs", line: 3, col: nil) == ["+3", "/a/b.rs"])
        #expect(PaneController.editorArguments("mate", path: "/a/b.rs", line: 3, col: nil) == ["/a/b.rs"])
    }
}
