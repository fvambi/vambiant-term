import Foundation
import Testing
@testable import VambiantTerm

struct JSONValueTests {
    @Test func decodesAndLooksUpPaths() throws {
        let data = Data(#"{"font":{"size":13.5,"fallback":["a","b"],"ligatures":true},"api":{"port":7433}}"#.utf8)
        let v = try JSONDecoder().decode(JSONValue.self, from: data)
        #expect(v[path: "font.size"]?.doubleValue == 13.5)
        #expect(v[path: "font.fallback"]?.stringArray == ["a", "b"])
        #expect(v[path: "font.ligatures"]?.boolValue == true)
        #expect(v[path: "api.port"]?.display == "7433")
        #expect(v[path: "font.nope"] == nil)
    }

    @Test func encodesWholeNumbersAsIntegers() throws {
        let text = try String(data: JSONEncoder().encode(JSONValue.number(15)), encoding: .utf8)
        #expect(text == "15")
        let frac = try String(data: JSONEncoder().encode(JSONValue.number(14.5)), encoding: .utf8)
        #expect(frac == "14.5")
    }
}

struct ChordParsingTests {
    @Test func parsesTheDaemonsCanonicalSpelling() {
        #expect(KeyChord.parse("shift+cmd+a") == KeyChord("a", command: true, shift: true))
        #expect(KeyChord.parse("cmd+.") == KeyChord(".", command: true))
        #expect(KeyChord.parse("ctrl+b") == KeyChord("b", control: true))
        #expect(KeyChord.parse("alt+cmd+left") == KeyChord("left", command: true, option: true))
        #expect(KeyChord.parse("shift+cmd+return") == KeyChord("enter", command: true, shift: true))
        #expect(KeyChord.parse("hyper+x") == nil)
        #expect(KeyChord.parse("") == nil)
    }

    @Test func buildsAKeymapFromTheResolvedTable() {
        let resolved = ResolvedKeymap(
            profile: "both", prefix: "ctrl+a",
            bindings: [
                KeyBinding(chord: "cmd+j", action: "inbox.open", source: "keymap.toml"),
                KeyBinding(chord: "prefix shift+n", action: "task.new", source: "tmux"),
                KeyBinding(chord: "cmd+t", action: "tab.new", source: "macos"),
                KeyBinding(chord: "bogus++", action: "tab.new", source: "macos"),
            ],
            errors: [], conflicts: []
        )
        let actions = [
            ActionMeta(id: "inbox.open", label: "Approval inbox", milestone: "M5"),
            ActionMeta(id: "tab.new", label: "New tab", milestone: "M4"),
        ]
        var km = Keymap(resolved: resolved, actions: actions)
        #expect(km.resolve(KeyChord("t", command: true)) == .action(.newTab))
        #expect(km.resolve(KeyChord("a", control: true)) == .prefixArmed, "the configured prefix, not C-b")
        #expect(km.resolve(KeyChord("n", shift: true)) == .action(.unavailable("task.new (task.new, ?)")))
        if case .action(.unavailable(let what)) = km.resolve(KeyChord("j", command: true)) {
            #expect(what.contains("Approval inbox"))
        } else {
            Issue.record("cmd+j must be the named unavailable action")
        }
        #expect(km.isCommandBinding(KeyChord("t", command: true)))
        #expect(!km.isCommandBinding(KeyChord("q", command: true)))
    }
}

struct ThemeFileTests {
    @Test func buildsAThemeFromAFile() throws {
        let json = ##"""
        {"name":"t","background":"#0d0f12","foreground":"#d8dee9","cursor":"#7aa2f7","selection":"#2a2f3a",
         "normal":{"black":"#1a1d23","red":"#e06c75","green":"#98c379","yellow":"#e5c07b",
                   "blue":"#61afef","magenta":"#c678dd","cyan":"#56b6c2","white":"#abb2bf"},
         "bright":{"black":"#4b5263","red":"#ff7b86","green":"#a9d977","yellow":"#f0d18a",
                   "blue":"#79c0ff","magenta":"#d7a3ff","cyan":"#6fd3de","white":"#e6e9ef"},
         "ui":{"accent":"#7aa2f7","warning":"#e5c07b","danger":"#e06c75","success":"#98c379"}}
        """##
        let file = try JSONDecoder().decode(ThemeFile.self, from: Data(json.utf8))
        let theme = try #require(Theme(file: file))
        #expect(theme.background == Theme.vambiantDark.background)
        #expect(theme.palette[9] == RGBA(hex: 0xFF7B86))
        #expect(theme.palette.count == 256)
        var broken = file
        broken.cursor = "blue"
        #expect(Theme(file: broken) == nil)
    }
}

struct ShellConfigTests {
    @Test func decodesTheKeysTheShellHonours() throws {
        let json = #"""
        {"font":{"family":"Menlo","fallback":[],"size":12,"line_height":1.1,"cell_width":1,"ligatures":true,
                 "ligature_disable":[],"thin_strokes":"auto","bold_is_bright":true},
         "theme":{"name":"vambiant-dark","light":"vambiant-light","follow_system":false},
         "window":{"padding":{"x":4,"y":2},"opacity":1},
         "cursor":{"style":"bar","blink":false,"blink_interval_ms":600},
         "mux":{"keymap_profile":"both","prefix":"ctrl+a","detach_on_close":false,"default_layout":"single"},
         "blocks":{"dividers":false,"failed_tint":true,"sticky_header":true},
         "extra":{"ignored":1}}
        """#
        let v = try JSONDecoder().decode(JSONValue.self, from: Data(json.utf8))
        let c = try v.decode(ShellConfig.self)
        #expect(c.font.family == "Menlo")
        #expect(c.font.boldIsBright)
        #expect(c.cursor.style == "bar")
        #expect(c.mux.prefix == "ctrl+a")
        #expect(!c.mux.detachOnClose)
        #expect(!c.blocks.dividers && c.blocks.failedTint)
        #expect(c.window.padding.x == 4)
    }
}
