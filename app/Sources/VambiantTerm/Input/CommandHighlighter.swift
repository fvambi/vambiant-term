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
