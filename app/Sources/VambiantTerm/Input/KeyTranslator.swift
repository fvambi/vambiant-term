// NSEvent → VtKeyEvent. Physical keys map to `vt_core::key::KeyCode`
// numeric values (the W3C `code` set, stable across the ABI); the daemon's
// encoder turns them into bytes using the session's live keyboard modes,
// so nothing here knows about kitty or modifyOtherKeys.

import AppKit
import CVambiantTerm

enum KeyMods {
    static let shift: UInt16 = 1 << 0
    static let ctrl: UInt16 = 1 << 1
    static let alt: UInt16 = 1 << 2
    static let superKey: UInt16 = 1 << 3
    static let capsLock: UInt16 = 1 << 4
    static let numLock: UInt16 = 1 << 5
}

enum KeyTranslator {
    /// `vt_core::key::KeyCode` values, keyed by the macOS virtual key code
    /// (Carbon `kVK_*`, which is layout-independent and therefore physical).
    static let codeByVirtualKey: [UInt16: UInt16] = [
        0x32: 1, // Backquote
        0x2A: 2, // Backslash
        0x21: 3, // BracketLeft
        0x1E: 4, // BracketRight
        0x2B: 5, // Comma
        0x1D: 6, // Digit0
        0x12: 7, // Digit1
        0x13: 8, // Digit2
        0x14: 9, // Digit3
        0x15: 10, // Digit4
        0x17: 11, // Digit5
        0x16: 12, // Digit6
        0x1A: 13, // Digit7
        0x1C: 14, // Digit8
        0x19: 15, // Digit9
        0x18: 16, // Equal
        0x00: 20, // A
        0x0B: 21, // B
        0x08: 22, // C
        0x02: 23, // D
        0x0E: 24, // E
        0x03: 25, // F
        0x05: 26, // G
        0x04: 27, // H
        0x22: 28, // I
        0x26: 29, // J
        0x28: 30, // K
        0x25: 31, // L
        0x2E: 32, // M
        0x2D: 33, // N
        0x1F: 34, // O
        0x23: 35, // P
        0x0C: 36, // Q
        0x0F: 37, // R
        0x01: 38, // S
        0x11: 39, // T
        0x20: 40, // U
        0x09: 41, // V
        0x0D: 42, // W
        0x07: 43, // X
        0x10: 44, // Y
        0x06: 45, // Z
        0x1B: 46, // Minus
        0x2F: 47, // Period
        0x27: 48, // Quote
        0x29: 49, // Semicolon
        0x2C: 50, // Slash
        0x33: 53, // Backspace
        0x24: 58, // Enter
        0x31: 63, // Space
        0x30: 64, // Tab
        0x75: 68, // Delete
        0x77: 69, // End
        0x73: 71, // Home
        0x72: 72, // Insert
        0x79: 73, // PageDown
        0x74: 74, // PageUp
        0x7D: 75, // ArrowDown
        0x7B: 76, // ArrowLeft
        0x7C: 77, // ArrowRight
        0x7E: 78, // ArrowUp
        0x4C: 97, // NumpadEnter
        0x35: 120, // Escape
        0x7A: 121, // F1
        0x78: 122, // F2
        0x63: 123, // F3
        0x76: 124, // F4
        0x60: 125, // F5
        0x61: 126, // F6
        0x62: 127, // F7
        0x64: 128, // F8
        0x65: 129, // F9
        0x6D: 130, // F10
        0x67: 131, // F11
        0x6F: 132, // F12
    ]

    static func mods(from flags: NSEvent.ModifierFlags) -> UInt16 {
        var m: UInt16 = 0
        if flags.contains(.shift) {
            m |= KeyMods.shift
        }
        if flags.contains(.control) {
            m |= KeyMods.ctrl
        }
        if flags.contains(.option) {
            m |= KeyMods.alt
        }
        if flags.contains(.command) {
            m |= KeyMods.superKey
        }
        if flags.contains(.capsLock) {
            m |= KeyMods.capsLock
        }
        if flags.contains(.numericPad) {
            m |= KeyMods.numLock
        }
        return m
    }

    /// Builds the flat event. `text` is what the key produced after the
    /// keyboard layout and dead keys (empty for a bare modifier or a
    /// function key); `unshifted` is the layout's base character.
    static func event(
        action: UInt8,
        virtualKey: UInt16,
        modifierFlags: NSEvent.ModifierFlags,
        text: String,
        unshifted: String
    ) -> VtKeyEvent {
        var ev = VtKeyEvent()
        ev.action = action
        ev.key = codeByVirtualKey[virtualKey] ?? 0
        ev.mods = mods(from: modifierFlags)
        // Control/Command produce control characters or nothing in
        // `characters`; the daemon derives the byte from key + mods, so
        // only pass printable text through.
        let bytes = Array(text.utf8)
        if !bytes.isEmpty, bytes.count <= 8, bytes.first.map({ $0 >= 0x20 && $0 != 0x7F }) == true {
            withUnsafeMutableBytes(of: &ev.utf8) { dst in
                for (i, b) in bytes.enumerated() {
                    dst[i] = b
                }
            }
            ev.utf8_len = UInt8(bytes.count)
        }
        ev.unshifted = unshifted.unicodeScalars.first.map(\.value) ?? 0
        return ev
    }

    @MainActor
    static func event(from e: NSEvent, action: UInt8 = 0) -> VtKeyEvent {
        event(
            action: action,
            virtualKey: e.keyCode,
            modifierFlags: e.modifierFlags,
            text: e.characters ?? "",
            unshifted: e.charactersIgnoringModifiers ?? ""
        )
    }
}
