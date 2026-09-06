// What `config.get` returns, decoded. Field metadata comes from
// `vt_config::describe` and drives the Settings window; the app never
// hardcodes a key it did not learn from there.

import Foundation

struct FieldMeta: Decodable, Identifiable, Sendable, Hashable {
    let path: String
    let doc: String
    let kind: String
    let min: Double?
    let max: Double?
    let step: Double?
    let options: [String]?
    let applied: String
    let milestone: String?

    var id: String {
        path
    }

    /// `font` of `font.size`; `agents` of `agents.claude.binary`.
    var section: String {
        String(path.split(separator: ".").first ?? "")
    }

    /// The rest, `size` or `claude.binary`.
    var name: String {
        path.split(separator: ".").dropFirst().joined(separator: ".")
    }

    var appliesLater: Bool {
        applied == "later"
    }
}

struct ActionMeta: Decodable, Identifiable, Sendable, Hashable {
    let id: String
    let label: String
    let milestone: String
}

struct ConfigFileError: Decodable, Sendable, Hashable, CustomStringConvertible {
    let file: String
    let line: Int?
    let message: String
    var description: String {
        "\(file)\(line.map { ":\($0)" } ?? ""): \(message)"
    }
}

struct KeyBinding: Decodable, Identifiable, Sendable, Hashable {
    let chord: String
    let action: String
    let source: String
    var id: String {
        chord
    }
}

struct ResolvedKeymap: Decodable, Sendable, Hashable {
    let profile: String
    let prefix: String
    let bindings: [KeyBinding]
    let errors: [String]
    let conflicts: [String]
}

struct AnsiFile: Codable, Sendable, Hashable {
    var black, red, green, yellow, blue, magenta, cyan, white: String
    var ordered: [String] {
        [black, red, green, yellow, blue, magenta, cyan, white]
    }
}

struct UiFile: Codable, Sendable, Hashable {
    var accent, warning, danger, success: String
}

struct ThemeFile: Codable, Identifiable, Sendable, Hashable {
    var name: String
    var background, foreground, cursor, selection: String
    var normal: AnsiFile
    var bright: AnsiFile
    var ui: UiFile
    var id: String {
        name
    }
}

struct ConfigPaths: Decodable, Sendable, Hashable {
    let config: String
    let keymap: String
    let themes: String
}

struct ConfigSnapshot: Decodable, Sendable {
    let paths: ConfigPaths
    let config: JSONValue
    let configError: ConfigFileError?
    let configWarnings: [String]
    let keymap: ResolvedKeymap
    let keymapError: ConfigFileError?
    let themes: [String: ThemeFile]
    let themeWarnings: [String]
    let fields: [FieldMeta]
    let actions: [ActionMeta]
    let defaults: JSONValue

    enum CodingKeys: String, CodingKey {
        case paths, config, keymap, themes, fields, actions, defaults
        case configError = "config_error"
        case configWarnings = "config_warnings"
        case keymapError = "keymap_error"
        case themeWarnings = "theme_warnings"
    }

    /// Section ids in the reference's order.
    var sections: [String] {
        var seen: [String] = []
        for f in fields where !seen.contains(f.section) {
            seen.append(f.section)
        }
        return seen
    }
}

/// The keys the shell itself honours, typed.
struct ShellFont: Decodable, Equatable, Sendable {
    var family: String
    var fallback: [String]
    var size: Double
    var lineHeight: Double
    var boldIsBright: Bool
    enum CodingKeys: String, CodingKey {
        case family, fallback, size
        case lineHeight = "line_height"
        case boldIsBright = "bold_is_bright"
    }
}

struct ShellThemeRef: Decodable, Equatable, Sendable {
    var name: String
    var light: String
    var followSystem: Bool
    enum CodingKeys: String, CodingKey {
        case name, light
        case followSystem = "follow_system"
    }
}

struct ShellPadding: Decodable, Equatable, Sendable {
    var x: Double
    var y: Double
}

struct ShellWindow: Decodable, Equatable, Sendable {
    var padding: ShellPadding
}

struct ShellCursor: Decodable, Equatable, Sendable {
    var style: String
    var blink: Bool
    var blinkIntervalMs: Int
    enum CodingKeys: String, CodingKey {
        case style, blink
        case blinkIntervalMs = "blink_interval_ms"
    }
}

struct ShellMux: Decodable, Equatable, Sendable {
    var keymapProfile: String
    var prefix: String
    var detachOnClose: Bool
    enum CodingKeys: String, CodingKey {
        case prefix
        case keymapProfile = "keymap_profile"
        case detachOnClose = "detach_on_close"
    }
}

struct ShellTerminal: Decodable, Equatable, Sendable {
    var copyOnSelect: Bool
    enum CodingKeys: String, CodingKey {
        case copyOnSelect = "copy_on_select"
    }
}

struct ShellBlocks: Decodable, Equatable, Sendable {
    var dividers: Bool
    var failedTint: Bool
    var stickyHeader: Bool
    enum CodingKeys: String, CodingKey {
        case dividers
        case failedTint = "failed_tint"
        case stickyHeader = "sticky_header"
    }
}

struct ShellInput: Decodable, Equatable, Sendable {
    var mode: String
}

struct ShellEditor: Decodable, Equatable, Sendable {
    var program: String
}

struct ShellNotifications: Decodable, Equatable, Sendable {
    var awaitingInput: Bool
    var agentFinished: String
    var agentCrashed: Bool
    var longCommandMs: Int
    var coalesceWindowMs: Int
    enum CodingKeys: String, CodingKey {
        case awaitingInput = "awaiting_input"
        case agentFinished = "agent_finished"
        case agentCrashed = "agent_crashed"
        case longCommandMs = "long_command_ms"
        case coalesceWindowMs = "coalesce_window_ms"
    }
}

struct ShellConfig: Decodable, Equatable, Sendable {
    var font: ShellFont
    var theme: ShellThemeRef
    var window: ShellWindow
    var cursor: ShellCursor
    var mux: ShellMux
    var blocks: ShellBlocks
    var input: ShellInput
    /// Absent on daemons older than `copy_on_select`.
    var terminal: ShellTerminal?
    /// Absent on daemons older than `[editor]`.
    var editor: ShellEditor?
    var notifications: ShellNotifications?
}

struct SetParams: Encodable {
    let key: String
    let value: JSONValue
}

struct BindParams: Encodable {
    let chord: String
    let action: String?
}

struct ThemeParams: Encodable {
    let theme: ThemeFile
}
