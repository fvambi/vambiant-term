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

    /// The main repository root for `cwd`: the directory holding `.git`,
    /// or for a worktree the main repository the worktree belongs to
    /// (`<gitdir>/commondir`). Nil outside a repository.
    static func repoRoot(for cwd: String) -> String? {
        guard let gitDir = gitDirectory(startingAt: cwd) else { return nil }
        let common = gitDir + "/commondir"
        if let rel = try? String(contentsOfFile: common, encoding: .utf8) {
            let target = rel.trimmingCharacters(in: .whitespacesAndNewlines)
            let base = URL(fileURLWithPath: gitDir)
            let main = target.hasPrefix("/") ? URL(fileURLWithPath: target) : base.appendingPathComponent(target)
            return main.standardizedFileURL.deletingLastPathComponent().path
        }
        return URL(fileURLWithPath: gitDir).deletingLastPathComponent().path
    }

    /// Lines added and removed from `git diff --numstat` output.
    static func diffStats(numstat: String) -> (added: Int, removed: Int) {
        var added = 0
        var removed = 0
        for line in numstat.split(separator: "\n") {
            let cols = line.split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false)
            guard cols.count >= 2 else { continue }
            added += Int(cols[0]) ?? 0 // "-" for binary files → 0
            removed += Int(cols[1]) ?? 0
        }
        return (added, removed)
    }

    /// Runs `git diff --numstat` in `cwd` off the main thread and reports
    /// the totals; nil when git is unavailable or `cwd` is not a repo.
    static func diffStats(for cwd: String, completion: @escaping @Sendable ((added: Int, removed: Int)?) -> Void) {
        DispatchQueue.global(qos: .utility).async {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
            p.arguments = ["git", "-C", cwd, "diff", "--numstat"]
            let out = Pipe()
            p.standardOutput = out
            p.standardError = FileHandle.nullDevice
            do {
                try p.run()
            } catch {
                completion(nil)
                return
            }
            let data = out.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
            guard p.terminationStatus == 0 else {
                completion(nil)
                return
            }
            completion(diffStats(numstat: String(bytes: data, encoding: .utf8) ?? ""))
        }
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
