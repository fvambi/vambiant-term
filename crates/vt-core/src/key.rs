//! Keyboard input encoding: a flat, FFI-friendly key event in, terminal
//! bytes out. Legacy xterm sequences, `modifyOtherKeys` and the kitty
//! keyboard protocol (push/pop/query, all five flag bits) are handled by the
//! backend's encoder, which reads the live terminal modes, so the caller never
//! tracks protocol state.

/// Key press/release/repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Key went down.
    Press,
    /// Key came up (only reported to programs that asked for it).
    Release,
    /// Auto-repeat.
    Repeat,
}

/// Modifier bits. Values match the W3C-style order the renderer uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyMods(pub u16);

impl KeyMods {
    /// Shift.
    pub const SHIFT: u16 = 1 << 0;
    /// Control.
    pub const CTRL: u16 = 1 << 1;
    /// Alt / Option.
    pub const ALT: u16 = 1 << 2;
    /// Command / Super.
    pub const SUPER: u16 = 1 << 3;
    /// Caps Lock engaged.
    pub const CAPS_LOCK: u16 = 1 << 4;
    /// Num Lock engaged.
    pub const NUM_LOCK: u16 = 1 << 5;
}

/// Physical key, as the W3C `KeyboardEvent.code` set. The numeric value is
/// stable across the FFI boundary; the backend maps it to its own enum.
///
/// Only the keys a terminal must distinguish are named here; everything the
/// renderer cannot map sends [`KeyCode::Unidentified`] plus `utf8` text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
#[allow(missing_docs)]
pub enum KeyCode {
    Unidentified = 0,
    Backquote = 1,
    Backslash = 2,
    BracketLeft = 3,
    BracketRight = 4,
    Comma = 5,
    Digit0 = 6,
    Digit1 = 7,
    Digit2 = 8,
    Digit3 = 9,
    Digit4 = 10,
    Digit5 = 11,
    Digit6 = 12,
    Digit7 = 13,
    Digit8 = 14,
    Digit9 = 15,
    Equal = 16,
    A = 20,
    B = 21,
    C = 22,
    D = 23,
    E = 24,
    F = 25,
    G = 26,
    H = 27,
    I = 28,
    J = 29,
    K = 30,
    L = 31,
    M = 32,
    N = 33,
    O = 34,
    P = 35,
    Q = 36,
    R = 37,
    S = 38,
    T = 39,
    U = 40,
    V = 41,
    W = 42,
    X = 43,
    Y = 44,
    Z = 45,
    Minus = 46,
    Period = 47,
    Quote = 48,
    Semicolon = 49,
    Slash = 50,
    Backspace = 53,
    Enter = 58,
    Space = 63,
    Tab = 64,
    Delete = 68,
    End = 69,
    Home = 71,
    Insert = 72,
    PageDown = 73,
    PageUp = 74,
    ArrowDown = 75,
    ArrowLeft = 76,
    ArrowRight = 77,
    ArrowUp = 78,
    NumpadEnter = 97,
    Escape = 120,
    F1 = 121,
    F2 = 122,
    F3 = 123,
    F4 = 124,
    F5 = 125,
    F6 = 126,
    F7 = 127,
    F8 = 128,
    F9 = 129,
    F10 = 130,
    F11 = 131,
    F12 = 132,
}

/// One key event from the renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// Press / release / repeat.
    pub action: KeyAction,
    /// Physical key.
    pub key: KeyCode,
    /// Modifier state.
    pub mods: KeyMods,
    /// Text the key produces with the current layout and modifiers, before
    /// any Ctrl/Meta transformation. `None` for keys without text.
    pub utf8: Option<String>,
    /// The character the key produces without Shift, when known (needed for
    /// the kitty protocol's alternate-key reporting).
    pub unshifted: Option<char>,
}

impl KeyEvent {
    /// A plain key press with optional text.
    pub fn press(key: KeyCode, mods: u16, utf8: Option<&str>) -> Self {
        Self {
            action: KeyAction::Press,
            key,
            mods: KeyMods(mods),
            utf8: utf8.map(str::to_owned),
            unshifted: None,
        }
    }
}
