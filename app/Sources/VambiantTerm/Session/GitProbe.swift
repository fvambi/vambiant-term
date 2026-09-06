// The git facts the chips and block headers show, read straight from the
// repository files (no `git` process per prompt): the current branch from
// `.git/HEAD`, found by walking up from a directory. Worktrees keep a
// `.git` *file* pointing at the real gitdir; that is followed too.

import Foundation

enum GitProbe {
    /// Branch name for `cwd` (`main`), a short commit id when detached, or
    /// nil outside a repository.
    static func branch(for cwd: String) -> String? {
        guard let gitDir = gitDirectory(startingAt: cwd) else { return nil }
        guard let head = try? String(contentsOfFile: gitDir + "/HEAD", encoding: .utf8) else { return nil }
        return branch(fromHead: head)
    }

    static func branch(fromHead head: String) -> String? {
        let line = head.trimmingCharacters(in: .whitespacesAndNewlines)
        if line.hasPrefix("ref: refs/heads/") {
            return String(line.dropFirst("ref: refs/heads/".count))
        }
        if line.hasPrefix("ref: ") {
            return String(line.dropFirst(5))
        }
        return line.isEmpty ? nil : String(line.prefix(8))
    }

    /// The `.git` directory governing `path`, following worktree `.git`
    /// files (`gitdir: <path>`). Stops at the filesystem root.
    static func gitDirectory(startingAt path: String) -> String? {
        var dir = URL(fileURLWithPath: path).standardizedFileURL
        for _ in 0 ..< 64 {
            let candidate = dir.appendingPathComponent(".git").path
            var isDir: ObjCBool = false
            if FileManager.default.fileExists(atPath: candidate, isDirectory: &isDir) {
                if isDir.boolValue {
                    return candidate
                }
                if let text = try? String(contentsOfFile: candidate, encoding: .utf8),
                   text.hasPrefix("gitdir: ") {
                    let target = text.dropFirst(8).trimmingCharacters(in: .whitespacesAndNewlines)
                    return target.hasPrefix("/") ? target : dir.appendingPathComponent(target).standardizedFileURL.path
                }
            }
            let parent = dir.deletingLastPathComponent()
            if parent.path == dir.path {
                return nil
            }
            dir = parent
        }
        return nil
    }

    /// `~/code/app` for a path under the home directory.
    static func abbreviated(_ path: String) -> String {
        let home = NSHomeDirectory()
        if path == home {
            return "~"
        }
        if path.hasPrefix(home + "/") {
            return "~" + path.dropFirst(home.count)
        }
        return path
    }
}
