import AppKit
import CVambiantTerm
import Testing
@testable import VambiantTerm

struct KeyTranslatorTests {
    @Test func lettersCarryTextAndPhysicalCode() {
        let ev = KeyTranslator.event(action: 0, virtualKey: 0x00, modifierFlags: [], text: "a", unshifted: "a")
        #expect(ev.key == 20) // KeyCode::A
        #expect(ev.utf8_len == 1)
        #expect(ev.utf8.0 == UInt8(ascii: "a"))
        #expect(ev.unshifted == UInt32(UInt8(ascii: "a")))
        #expect(ev.mods == 0)
    }

    @Test func controlKeysCarryNoText() {
        let ev = KeyTranslator.event(action: 0, virtualKey: 0x08, modifierFlags: [.control], text: "\u{03}", unshifted: "c")
        #expect(ev.key == 22) // KeyCode::C
        #expect(ev.utf8_len == 0, "the daemon derives ^C from key + mods")
        #expect(ev.mods == KeyMods.ctrl)
    }

    @Test func specialKeysMap() {
        #expect(KeyTranslator.codeByVirtualKey[0x24] == 58) // Enter
        #expect(KeyTranslator.codeByVirtualKey[0x35] == 120) // Escape
        #expect(KeyTranslator.codeByVirtualKey[0x7B] == 76) // ArrowLeft
        #expect(KeyTranslator.codeByVirtualKey[0x33] == 53) // Backspace
        #expect(KeyTranslator.codeByVirtualKey[0x6F] == 132) // F12
    }

    @Test func unknownKeysAreUnidentifiedButKeepText() {
        let ev = KeyTranslator.event(action: 0, virtualKey: 0xFF, modifierFlags: [], text: "é", unshifted: "e")
        #expect(ev.key == 0)
        #expect(ev.utf8_len == 2)
    }

    @Test func modifiersUseTheSharedBitLayout() {
        let m = KeyTranslator.mods(from: [.shift, .command, .option])
        #expect(m == KeyMods.shift | KeyMods.superKey | KeyMods.alt)
    }
}

struct KeymapTests {
    @Test func commandChordsResolveDirectly() {
        var km = Keymap()
        #expect(km.resolve(KeyChord("d", command: true)) == .action(.splitRight))
        #expect(km.resolve(KeyChord("d", command: true, shift: true)) == .action(.splitDown))
        #expect(km.resolve(KeyChord("t", command: true)) == .action(.newTab))
    }

    @Test func prefixArmsThenConsumesOneKey() {
        var km = Keymap()
        #expect(km.resolve(km.prefix) == .prefixArmed)
        #expect(km.resolve(KeyChord("%", shift: true)) == .action(.splitRight))
        #expect(km.resolve(KeyChord("%", shift: true)) == .passthrough, "prefix is single-shot")
    }

    @Test func doublePrefixSendsALiteralPrefix() {
        var km = Keymap()
        _ = km.resolve(km.prefix)
        #expect(km.resolve(km.prefix) == .action(.sendPrefix))
    }

    @Test func unknownPrefixedKeyPassesThrough() {
        var km = Keymap()
        _ = km.resolve(km.prefix)
        #expect(km.resolve(KeyChord("q")) == .passthrough)
    }

    @Test func unimplementedBindingsAreNamedNotSilent() {
        var km = Keymap()
        #expect(km.resolve(KeyChord("a", command: true, shift: true)) == .action(.inboxOpen))
        #expect(km.resolve(KeyChord("k", command: true)) == .action(.askAgent))
        // An action the daemon lists but the shell has not built resolves
        // to a named unavailable action, never to silence or the terminal.
        let unbuilt = ShellAction.from(id: "task.new", label: "New agent task (worktree)", milestone: "M6")
        #expect(unbuilt == .unavailable("New agent task (worktree) (task.new, M6)"))
    }
}

struct ABITests {
    @Test func libraryMatchesHeader() {
        #expect(vt_ffi_abi_version() == UInt32(VtABI_VERSION))
    }

    @Test func defaultSocketIsUnderTheRuntimeDir() {
        let s = DaemonClient.defaultSocket()
        #expect(s.hasSuffix("vtermd.sock"))
    }
}
