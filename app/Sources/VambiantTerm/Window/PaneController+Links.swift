// ⌘-click targets (12 §E4): URLs go to the browser; files go to
// `[editor] program` with the editor's own `path:line:col` form, else to
// whatever macOS opens them with. Relative paths resolve against the
// pane's cwd.

import AppKit

extension PaneController {
    func wireLinks() {
        view.onOpenLink = { [weak self] link in self?.open(link) }
    }

    func open(_ link: Link) {
        switch link.kind {
        case .url:
            if let url = URL(string: link.text) {
                NSWorkspace.shared.open(url)
            }
        case .path:
            let (raw, line, col) = link.pathAndPosition
            let expanded = (raw as NSString).expandingTildeInPath
            let path = expanded.hasPrefix("/") ? expanded : ((cwd ?? NSHomeDirectory()) as NSString).appendingPathComponent(expanded)
            guard FileManager.default.fileExists(atPath: path) else {
                container.input.setHint("no such file: \(path)")
                NSSound.beep()
                return
            }
            if let program = editorProgram, !program.isEmpty {
                Self.openInEditor(program, path: path, line: line, col: col)
            } else {
                NSWorkspace.shared.open(URL(fileURLWithPath: path))
            }
        }
    }

    /// `code -g path:line:col`, `zed path:line:col`, `idea --line N path`,
    /// `vim +N path`; anything else gets the bare path.
    nonisolated static func editorArguments(_ program: String, path: String, line: Int?, col: Int?) -> [String] {
        let name = (program as NSString).lastPathComponent
        let position = [line, col].compactMap(\.self).map(String.init).joined(separator: ":")
        let located = position.isEmpty ? path : "\(path):\(position)"
        switch name {
        case "code", "code-insiders", "cursor", "codium", "windsurf": return ["-g", located]
        case "zed", "subl", "sublime_text", "atom", "nova": return [located]
        case "idea", "webstorm", "pycharm", "goland", "rustrover", "clion":
            return line.map { ["--line", String($0), path] } ?? [path]
        case "vim", "nvim", "vi", "emacs", "nano", "hx", "micro":
            return line.map { ["+\($0)", path] } ?? [path]
        default: return [path]
        }
    }

    private static func openInEditor(_ program: String, path: String, line: Int?, col: Int?) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = [program] + editorArguments(program, path: path, line: line, col: col)
        do {
            try process.run()
        } catch {
            NSLog("cannot run editor %@: %@", program, "\(error)")
            NSWorkspace.shared.open(URL(fileURLWithPath: path))
        }
    }
}
