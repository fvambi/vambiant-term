// Syntax colouring for the Warp-mode editor: a small shell tokenizer that
// knows commands, flags, strings, variables, operators and comments. It is
// deliberately shallow (no expansion, no grammar); `vt-policy` owns real
// parsing. Pure, so it is tested without a text view.

import Foundation

enum ShellToken: Equatable, Sendable {
    case command, flag, string, variable, op, comment, arg
}

struct ShellSpan: Equatable, Sendable {
    let range: Range<Int> // unicode scalar offsets
    let kind: ShellToken
}

enum CommandHighlighter {
    /// Words the shell resolves itself, so no PATH lookup can vouch for them.
    static let builtins: Set<String> = [
        "alias", "bg", "bind", "break", "builtin", "case", "cd", "command", "continue", "declare", "dirs", "disown", "do",
        "done", "echo", "elif", "else", "enable", "esac", "eval", "exec", "exit", "export", "false", "fc", "fg", "fi", "for",
        "function", "getopts", "hash", "help", "history", "if", "in", "jobs", "kill", "let", "local", "logout", "popd",
        "printf", "pushd", "pwd", "read", "readonly", "return", "select", "set", "shift", "shopt", "source", "suspend",
        "test", "then", "time", "times", "trap", "true", "type", "typeset", "ulimit", "umask", "unalias", "unset",
        "until", "wait", "while", "which", "[", "[[", ".", ":", "{", "}", "!",
    ]

    /// Whether a command word can be vouched for: on PATH, a builtin or
    /// keyword, a path (`./x`, `/usr/bin/x`, `~/x`), or an assignment.
    /// Aliases and functions are not visible here, so they underline too;
    /// the hint says the underline is a guess.
    static func isKnown(_ word: String, known: Set<String>) -> Bool {
        if word.isEmpty || word.contains("/") || word.contains("=") || word.hasPrefix("$") || word.hasPrefix("\\") {
            return true
        }
        return known.contains(word) || builtins.contains(word)
    }

    static func spans(_ text: String) -> [ShellSpan] {
        let s = Array(text.unicodeScalars)
        var out: [ShellSpan] = []
        var i = 0
        var expectCommand = true
        func push(_ start: Int, _ end: Int, _ kind: ShellToken) {
            if end > start {
                out.append(ShellSpan(range: start ..< end, kind: kind))
            }
        }
        while i < s.count {
            let c = s[i]
            if c == " " || c == "\t" || c == "\n" {
                if c == "\n" {
                    expectCommand = true
                }
                i += 1
                continue
            }
            if c == "#" {
                let start = i
                while i < s.count, s[i] != "\n" {
                    i += 1
                }
                push(start, i, .comment)
                continue
            }
            if c == "'" || c == "\"" {
                let quote = c
                let start = i
                i += 1
                while i < s.count, s[i] != quote {
                    if s[i] == "\\", quote == "\"", i + 1 < s.count {
                        i += 1
                    }
                    i += 1
                }
                i = min(i + 1, s.count)
                push(start, i, .string)
                expectCommand = false
                continue
            }
            if c == "|" || c == "&" || c == ";" || c == ">" || c == "<" || c == "(" || c == ")" {
                let start = i
                while i < s.count, "|&;><()".unicodeScalars.contains(s[i]) {
                    i += 1
                }
                push(start, i, .op)
                // `>` / `<` / `2>` take a file, not a command; `|`, `;`, `&&` do.
                let text = String(String.UnicodeScalarView(s[start ..< i]))
                expectCommand = !text.allSatisfy { $0 == ">" || $0 == "<" }
                continue
            }
            // A bare word: command, flag, variable or argument.
            let start = i
            while i < s.count, !" \t\n|&;><()'\"#".unicodeScalars.contains(s[i]) {
                i += 1
            }
            let word = String(String.UnicodeScalarView(s[start ..< i]))
            let kind: ShellToken
            if word.hasPrefix("$") {
                kind = .variable
            } else if expectCommand {
                // `FOO=bar cmd`: an assignment keeps the next word a command.
                if word.contains("="), !word.hasPrefix("=") {
                    kind = .variable
                } else {
                    kind = .command
                    expectCommand = false
                }
            } else if word.hasPrefix("-"), word.count > 1 {
                kind = .flag
            } else {
                kind = .arg
            }
            push(start, i, kind)
        }
        return out
    }
}
