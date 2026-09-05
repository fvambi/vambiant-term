// Key chords → shell actions. The daemon resolves docs/06 §7's two
// profiles plus keymap.toml into one table (`vt_config::keymap`); this
// file parses that table and keeps the prefix state. The built-in table
// exists only for the moment before the daemon has answered, and for tests.

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
    case openSettings
    case unavailable(String)

    enum Direction: Equatable, Sendable { case left, right, up, down }

    /// From a `vt_config::keymap::ACTIONS` id.
    static func from(id: String, label: String, milestone: String) -> ShellAction {
        switch id {
        case "tab.new": return .newTab
        case "window.new": return .newWindow
        case "pane.split_right": return .splitRight
        case "pane.split_down": return .splitDown
        case "pane.focus_left": return .focus(.left)
        case "pane.focus_right": return .focus(.right)
        case "pane.focus_up": return .focus(.up)
        case "pane.focus_down": return .focus(.down)
        case "pane.zoom": return .zoomPane
        case "pane.close": return .closePane
        case "session.detach": return .detach
        case "agent.interrupt": return .interruptAgent
        case "prefix.send": return .sendPrefix
        case "settings.open": return .openSettings
        default: return .unavailable("\(label) (\(id), \(milestone))")
        }
    }
}

struct KeyChord: Hashable, Sendable {
    /// Lower-case key: a character, or `left`, `enter`, `escape`, `f5`…
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

    /// Parses the daemon's canonical spelling (`shift+cmd+a`, `cmd+.`,
    /// `ctrl+b`, `left`); returns nil for anything else.
    static func parse(_ text: String) -> KeyChord? {
        let parts = text.split(separator: "+", omittingEmptySubsequences: false).map(String.init)
        guard let last = parts.last, !last.isEmpty else { return nil }
        var command = false, shift = false, option = false, control = false
        for mod in parts.dropLast() {
            switch mod.lowercased() {
            case "cmd", "command", "super": command = true
            case "shift": shift = true
            case "alt", "opt", "option": option = true
            case "ctrl", "control": control = true
            default: return nil
            }
        }
        var key = last.lowercased()
        if key == "return" {
            key = "enter"
        }
        if key == "esc" {
            key = "escape"
        }
        return KeyChord(key, command: command, shift: shift, option: option, control: control)
    }
}

struct Keymap: Sendable {
    private var direct: [KeyChord: ShellAction]
    private var prefixed: [KeyChord: ShellAction]
    private(set) var prefix: KeyChord
    private(set) var awaitingPrefixKey = false

    /// The built-in defaults (`both` profiles, prefix C-b).
    init() {
        prefix = KeyChord("b", control: true)
        direct = [
            KeyChord("t", command: true): .newTab,
            KeyChord("n", command: true): .newWindow,
            KeyChord("d", command: true): .splitRight,
            KeyChord("d", command: true, shift: true): .splitDown,
            KeyChord("left", command: true, option: true): .focus(.left),
            KeyChord("right", command: true, option: true): .focus(.right),
            KeyChord("up", command: true, option: true): .focus(.up),
            KeyChord("down", command: true, option: true): .focus(.down),
            KeyChord("enter", command: true, shift: true): .zoomPane,
            KeyChord("w", command: true): .closePane,
            KeyChord(".", command: true): .interruptAgent,
            KeyChord(",", command: true): .openSettings,
            KeyChord("a", command: true, shift: true): .unavailable("Approval inbox (inbox.open, M5)"),
            KeyChord("p", command: true, shift: true): .unavailable("Command palette (palette.open, M5)"),
            KeyChord("k", command: true): .unavailable("⌘K assistant (ai.ask, M-AI)"),
        ]
        prefixed = [
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
            KeyChord("a"): .unavailable("Approval inbox (inbox.open, M5)"),
        ]
    }

    /// From the daemon's resolved table. Unparseable chords are skipped
    /// (the daemon already reported them).
    init(resolved: ResolvedKeymap, actions: [ActionMeta]) {
        prefix = KeyChord.parse(resolved.prefix) ?? KeyChord("b", control: true)
        direct = [:]
        prefixed = [:]
        for b in resolved.bindings {
            let meta = actions.first { $0.id == b.action }
            let action = ShellAction.from(id: b.action, label: meta?.label ?? b.action, milestone: meta?.milestone ?? "?")
            if b.chord.hasPrefix("prefix ") {
                if let c = KeyChord.parse(String(b.chord.dropFirst(7))) {
                    prefixed[c] = action
                }
            } else if let c = KeyChord.parse(b.chord) {
                direct[c] = action
            }
        }
    }

    enum Resolution: Equatable {
        case action(ShellAction)
        case prefixArmed
        case passthrough
    }

    /// True when a ⌘ chord is bound and must not reach the menu bar.
    func isCommandBinding(_ chord: KeyChord) -> Bool {
        chord.command && direct[chord] != nil
    }

    /// Consumes one chord. Prefix state lives here so the view stays dumb.
    mutating func resolve(_ chord: KeyChord) -> Resolution {
        if awaitingPrefixKey {
            awaitingPrefixKey = false
            if let action = prefixed[chord] {
                return .action(action)
            }
            return .passthrough
        }
        if chord == prefix {
            awaitingPrefixKey = true
            return .prefixArmed
        }
        if let action = direct[chord] {
            return .action(action)
        }
        return .passthrough
    }

    static func chord(from e: NSEvent) -> KeyChord {
        let flags = e.modifierFlags
        let key: String = switch e.keyCode {
        case 0x7B: "left"
        case 0x7C: "right"
        case 0x7E: "up"
        case 0x7D: "down"
        case 0x24, 0x4C: "enter"
        case 0x35: "escape"
        case 0x33: "backspace"
        case 0x30: "tab"
        case 0x31: "space"
        default:
            // Command swallows the shifted character on some layouts, so
            // read the unshifted one and carry shift as a flag; symbols
            // that need shift ("%", "\"") come through as themselves.
            (e.charactersIgnoringModifiers ?? "").lowercased()
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
