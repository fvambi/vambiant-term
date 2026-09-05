// The two shipped keymap profiles from docs/06 §7. Both are active
// ("both" in docs/09 `[mux] keymap_profile`) until `keymap.toml` lands
// with vt-config; the prefix is C-b. Actions the shell does not implement
// yet resolve to `.unavailable` so the key never silently reaches the pty.

import AppKit

enum ShellAction: Equatable, Sendable {
    case newTab, newWindow
    case splitRight, splitDown
    case focus(Direction)
    case zoomPane
    case detach
    case closePane
    case interruptAgent
    case sendPrefix
    case unavailable(String)

    enum Direction: Equatable, Sendable { case left, right, up, down }
}

struct KeyChord: Hashable, Sendable {
    /// Characters ignoring modifiers, lower-cased; arrows as "left"…
    let key: String
    let command: Bool
    let shift: Bool
    let option: Bool
    let control: Bool

    init(_ key: String, command: Bool = false, shift: Bool = false, option: Bool = false, control: Bool = false) {
        self.key = key
        self.command = command
        self.shift = shift
        self.option = option
        self.control = control
    }
}

struct Keymap: Sendable {
    /// macOS profile, ⌘-based.
    static let macos: [KeyChord: ShellAction] = [
        KeyChord("t", command: true): .newTab,
        KeyChord("n", command: true): .newWindow,
        KeyChord("d", command: true): .splitRight,
        KeyChord("d", command: true, shift: true): .splitDown,
        KeyChord("left", command: true, option: true): .focus(.left),
        KeyChord("right", command: true, option: true): .focus(.right),
        KeyChord("up", command: true, option: true): .focus(.up),
        KeyChord("down", command: true, option: true): .focus(.down),
        KeyChord("return", command: true, shift: true): .zoomPane,
        KeyChord("w", command: true): .closePane,
        KeyChord(".", command: true): .interruptAgent,
        KeyChord("a", command: true, shift: true): .unavailable("inbox.open (M5)"),
        KeyChord("p", command: true, shift: true): .unavailable("palette (M5)"),
        KeyChord("k", command: true): .unavailable("ai.ask (M-AI)"),
        KeyChord("f", command: true): .unavailable("scrollback search (M5)"),
        KeyChord("up", command: true): .unavailable("jump to previous prompt (M5)"),
        KeyChord("e", option: true): .unavailable("ai.explain_last_failure (M-AI)"),
        KeyChord("e", command: true, option: true): .unavailable("ai.show_last_payload (M-AI)"),
        KeyChord("n", command: true, shift: true): .unavailable("task.new (M6)"),
    ]

    /// tmux profile: the key after the prefix.
    static let tmux: [KeyChord: ShellAction] = [
        KeyChord("c"): .newTab,
        KeyChord("%", shift: true): .splitRight,
        KeyChord("\"", shift: true): .splitDown,
        KeyChord("left"): .focus(.left),
        KeyChord("right"): .focus(.right),
        KeyChord("up"): .focus(.up),
        KeyChord("down"): .focus(.down),
        KeyChord("z"): .zoomPane,
        KeyChord("d"): .detach,
        KeyChord("x"): .closePane,
        KeyChord("c", control: true): .interruptAgent,
        KeyChord("b", control: true): .sendPrefix,
        KeyChord("s"): .unavailable("session list (M5)"),
        KeyChord("a"): .unavailable("inbox.open (M5)"),
        KeyChord("a", shift: true): .unavailable("inbox.next_pending (M5)"),
        KeyChord(":", shift: true): .unavailable("palette (M5)"),
        KeyChord("k"): .unavailable("ai.ask (M-AI)"),
        KeyChord("e"): .unavailable("ai.explain_last_failure (M-AI)"),
        KeyChord("n", shift: true): .unavailable("task.new (M6)"),
        KeyChord("/"): .unavailable("scrollback search (M5)"),
        KeyChord("["): .unavailable("jump to previous prompt (M5)"),
    ]

    static let prefix = KeyChord("b", control: true)

    private(set) var awaitingPrefixKey = false

    enum Resolution: Equatable {
        case action(ShellAction)
        case prefixArmed
        case passthrough
    }

    /// Consumes one chord. Prefix state lives here so the view stays dumb.
    mutating func resolve(_ chord: KeyChord) -> Resolution {
        if awaitingPrefixKey {
            awaitingPrefixKey = false
            if let action = Self.tmux[chord] {
                return .action(action)
            }
            return .passthrough
        }
        if chord == Self.prefix {
            awaitingPrefixKey = true
            return .prefixArmed
        }
        if let action = Self.macos[chord] {
            return .action(action)
        }
        return .passthrough
    }

    static func chord(from e: NSEvent) -> KeyChord {
        let flags = e.modifierFlags
        let key: String
        switch e.keyCode {
        case 0x7B: key = "left"
        case 0x7C: key = "right"
        case 0x7E: key = "up"
        case 0x7D: key = "down"
        case 0x24, 0x4C: key = "return"
        case 0x35: key = "escape"
        case 0x33: key = "backspace"
        case 0x30: key = "tab"
        default:
            // Command swallows the shifted character on some layouts, so
            // read the unshifted one and carry shift as a flag; symbols
            // that need shift ("%", "\"") come through as themselves.
            let s = e.charactersIgnoringModifiers ?? ""
            key = s.lowercased()
        }
        return KeyChord(
            key,
            command: flags.contains(.command),
            shift: flags.contains(.shift),
            option: flags.contains(.option),
            control: flags.contains(.control)
        )
    }
}
