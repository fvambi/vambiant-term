//! Keyboard input encoding: a flat, FFI-friendly key event in, terminal
//! bytes out. Legacy xterm sequences, `modifyOtherKeys` and the kitty
//! keyboard protocol (push/pop/query, all five flag bits) are handled by the
//! backend's encoder, which reads the live terminal modes, so the caller never
//! tracks protocol state.

use serde::{Deserialize, Serialize};

/// Key press/release/repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAction {
    /// Key went down.
    Press,
    /// Key came up (only reported to programs that asked for it).
    Release,
    /// Auto-repeat.
    Repeat,
}

/// Modifier bits. Values match the W3C-style order the renderer uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

impl KeyCode {
    /// The key for a numeric code as it crosses the FFI boundary; unknown
    /// codes become [`KeyCode::Unidentified`].
    #[must_use]
    pub fn from_code(code: u16) -> Self {
        match code {
            0 => Self::Unidentified,
            1 => Self::Backquote,
            2 => Self::Backslash,
            3 => Self::BracketLeft,
            4 => Self::BracketRight,
            5 => Self::Comma,
            6 => Self::Digit0,
            7 => Self::Digit1,
            8 => Self::Digit2,
            9 => Self::Digit3,
            10 => Self::Digit4,
            11 => Self::Digit5,
            12 => Self::Digit6,
            13 => Self::Digit7,
            14 => Self::Digit8,
            15 => Self::Digit9,
            16 => Self::Equal,
            20 => Self::A,
            21 => Self::B,
            22 => Self::C,
            23 => Self::D,
            24 => Self::E,
            25 => Self::F,
            26 => Self::G,
            27 => Self::H,
            28 => Self::I,
            29 => Self::J,
            30 => Self::K,
            31 => Self::L,
            32 => Self::M,
            33 => Self::N,
            34 => Self::O,
            35 => Self::P,
            36 => Self::Q,
            37 => Self::R,
            38 => Self::S,
            39 => Self::T,
            40 => Self::U,
            41 => Self::V,
            42 => Self::W,
            43 => Self::X,
            44 => Self::Y,
            45 => Self::Z,
            46 => Self::Minus,
            47 => Self::Period,
            48 => Self::Quote,
            49 => Self::Semicolon,
            50 => Self::Slash,
            53 => Self::Backspace,
            58 => Self::Enter,
            63 => Self::Space,
            64 => Self::Tab,
            68 => Self::Delete,
            69 => Self::End,
            71 => Self::Home,
            72 => Self::Insert,
            73 => Self::PageDown,
            74 => Self::PageUp,
            75 => Self::ArrowDown,
            76 => Self::ArrowLeft,
            77 => Self::ArrowRight,
            78 => Self::ArrowUp,
            97 => Self::NumpadEnter,
            120 => Self::Escape,
            121 => Self::F1,
            122 => Self::F2,
            123 => Self::F3,
            124 => Self::F4,
            125 => Self::F5,
            126 => Self::F6,
            127 => Self::F7,
            128 => Self::F8,
            129 => Self::F9,
            130 => Self::F10,
            131 => Self::F11,
            132 => Self::F12,
            _ => Self::Unidentified,
        }
    }
}

/// One key event from the renderer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
