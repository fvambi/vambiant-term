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
    case promptPrevious, promptNext
    case blockSelectPrevious, blockSelectNext
    case blockExtendPrevious, blockExtendNext
    case blockTop, blockBottom
    case blockBookmarkPrevious, blockBookmarkNext
    case block(BlockAction)
    case clearScrollback
    case findOpen, findNext, findPrevious
    case stickyHeaderToggle
    case sidebarToggle
    case paletteOpen
    case askAgent, explainLastFailure
    case inboxOpen, inboxNext
    case mailboxOpen
    case historySearch, showLastPayload
    case scroll(ScrollStep)
    case unavailable(String)

    enum Direction: Equatable, Sendable { case left, right, up, down }
    enum ScrollStep: Equatable, Sendable { case pageUp, pageDown, top, bottom }

    /// From a `vt_config::keymap::ACTIONS` id.
    static func from(id: String, label: String, milestone: String) -> ShellAction {
        byId[id] ?? .unavailable("\(label) (\(id), \(milestone))")
    }

    private static let byId: [String: ShellAction] = [
        "tab.new": .newTab,
        "window.new": .newWindow,
        "pane.split_right": .splitRight,
        "pane.split_down": .splitDown,
        "pane.focus_left": .focus(.left),
        "pane.focus_right": .focus(.right),
        "pane.focus_up": .focus(.up),
        "pane.focus_down": .focus(.down),
        "pane.zoom": .zoomPane,
        "pane.close": .closePane,
        "session.detach": .detach,
        "agent.interrupt": .interruptAgent,
        "prefix.send": .sendPrefix,
        "settings.open": .openSettings,
        "prompt.previous": .promptPrevious,
        "prompt.next": .promptNext,
        "block.select_previous": .blockSelectPrevious,
        "block.select_next": .blockSelectNext,
        "block.extend_previous": .blockExtendPrevious,
        "block.extend_next": .blockExtendNext,
        "block.top": .blockTop,
        "block.bottom": .blockBottom,
        "block.bookmark": .block(.bookmark),
        "block.bookmark_previous": .blockBookmarkPrevious,
        "block.bookmark_next": .blockBookmarkNext,
        "block.copy_command": .block(.copyCommand),
        "block.copy_output": .block(.copyOutput),
        "block.copy_both": .block(.copyBoth),
        "block.reinput": .block(.reinput),
        "block.reinput_sudo": .block(.reinputSudo),
        "block.export": .block(.exportHTML),
        "block.menu": .block(.menu),
        "block.rerun": .block(.rerun),
        "scrollback.clear": .clearScrollback,
        "scrollback.search": .findOpen,
        "find.next": .findNext,
        "find.previous": .findPrevious,
        "block.sticky_toggle": .stickyHeaderToggle,
        "sidebar.toggle": .sidebarToggle,
        "palette.open": .paletteOpen,
        "ai.ask": .askAgent,
        "ai.explain_last_failure": .explainLastFailure,
        "inbox.open": .inboxOpen,
        "inbox.next_pending": .inboxNext,
        "mailbox.open": .mailboxOpen,
        "history.search": .historySearch,
        "ai.show_last_payload": .showLastPayload,
        "scrollback.page_up": .scroll(.pageUp),
        "scrollback.page_down": .scroll(.pageDown),
        "scrollback.top": .scroll(.top),
        "scrollback.bottom": .scroll(.bottom),
    ]
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
            KeyChord("up", command: true): .promptPrevious,
            KeyChord("down", command: true): .promptNext,
            KeyChord("up", command: true, control: true): .blockSelectPrevious,
            KeyChord("down", command: true, control: true): .blockSelectNext,
            KeyChord("up", command: true, shift: true, control: true): .blockExtendPrevious,
            KeyChord("down", command: true, shift: true, control: true): .blockExtendNext,
            KeyChord("up", command: true, shift: true): .blockTop,
            KeyChord("down", command: true, shift: true): .blockBottom,
            KeyChord("b", command: true): .block(.bookmark),
            KeyChord("up", option: true): .blockBookmarkPrevious,
            KeyChord("down", option: true): .blockBookmarkNext,
            KeyChord("c", command: true, shift: true): .block(.copyCommand),
            KeyChord("c", command: true, shift: true, option: true): .block(.copyOutput),
            KeyChord("i", command: true): .block(.reinput),
            KeyChord("i", command: true, shift: true): .block(.reinputSudo),
            KeyChord("m", control: true): .block(.menu),
            KeyChord("k", command: true, shift: true): .clearScrollback,
            KeyChord("f", command: true): .findOpen,
            KeyChord("g", command: true): .findNext,
            KeyChord("g", command: true, shift: true): .findPrevious,
            KeyChord("\\", command: true): .sidebarToggle,
            KeyChord("pageup", shift: true): .scroll(.pageUp),
            KeyChord("pagedown", shift: true): .scroll(.pageDown),
            KeyChord("home", shift: true): .scroll(.top),
            KeyChord("end", shift: true): .scroll(.bottom),
            KeyChord("a", command: true, shift: true): .inboxOpen,
            KeyChord("m", command: true, shift: true): .mailboxOpen,
            KeyChord("e", command: true, option: true): .showLastPayload,
            KeyChord("p", command: true, shift: true): .paletteOpen,
            KeyChord("k", command: true): .askAgent,
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
            KeyChord("a"): .inboxOpen,
            KeyChord("["): .promptPrevious,
            KeyChord("]"): .promptNext,
            KeyChord("pageup"): .scroll(.pageUp),
            KeyChord("pagedown"): .scroll(.pageDown),
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
        case 0x74: "pageup"
        case 0x79: "pagedown"
        case 0x73: "home"
        case 0x77: "end"
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
