//! Importing themes from other terminals (docs/09 "Themes", 12 §D1):
//! Warp YAML, Ghostty, Alacritty TOML, iTerm2 `.itermcolors` and base16
//! YAML. Each reader is a small purpose-built parser over the format's
//! documented shape — no YAML or plist crate — and every import is
//! contrast-checked afterwards (a warning, not a rejection).

use std::collections::BTreeMap;

use crate::theme::{Ansi8, Hex, Theme, Ui};

/// A source format, detected from the file name and its first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// `~/.warp/themes/*.yaml`: `accent`, `background`, `foreground`, `terminal_colors.normal/bright`.
    WarpYaml,
    /// Ghostty config or theme file: `background = …`, `palette = N=…`.
    Ghostty,
    /// Alacritty TOML: `[colors.primary]`, `[colors.normal]`, `[colors.bright]`.
    Alacritty,
    /// iTerm2 `.itermcolors` plist.
    Iterm2,
    /// base16 / tinted-theming YAML: `base00`…`base0F`.
    Base16,
}

/// Why a file could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// Nothing recognised the file.
    #[error(
        "cannot tell the theme format of `{0}`: expected a Warp .yaml, Ghostty config, Alacritty .toml, iTerm2 .itermcolors or base16 .yaml"
    )]
    UnknownFormat(String),
    /// A colour the format requires is missing.
    #[error("{format:?} theme is missing `{key}`")]
    Missing {
        /// The format.
        format: Format,
        /// The key.
        key: String,
    },
    /// A value is not a colour.
    #[error("{format:?} theme: `{key}` = {value:?} is not a colour")]
    BadColour {
        /// The format.
        format: Format,
        /// The key.
        key: String,
        /// The value.
        value: String,
    },
}

/// Detects the format from the file name and content.
pub fn detect(filename: &str, text: &str) -> Option<Format> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".itermcolors") || text.contains("<plist") {
        return Some(Format::Iterm2);
    }
    if text.contains("base00") && text.contains("base0F")
        || text.contains("base0f") && text.contains("base00")
    {
        return Some(Format::Base16);
    }
    if text.contains("terminal_colors") {
        return Some(Format::WarpYaml);
    }
    let ext = std::path::Path::new(filename)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    if ext.as_deref() == Some("toml")
        || text.contains("[colors.primary]")
        || text.contains("[colors.normal]")
    {
        return Some(Format::Alacritty);
    }
    if text.lines().any(|l| {
        l.trim_start().starts_with("palette")
            || l.trim_start().starts_with("background =")
            || l.trim_start().starts_with("background=")
    }) {
        return Some(Format::Ghostty);
    }
    None
}

/// Imports `text` as `name`, detecting the format unless `format` is given.
pub fn import(
    text: &str,
    filename: &str,
    name: &str,
    format: Option<Format>,
) -> Result<Theme, ImportError> {
    let format = format
        .or_else(|| detect(filename, text))
        .ok_or_else(|| ImportError::UnknownFormat(filename.to_owned()))?;
    let name = if name.trim().is_empty() {
        std::path::Path::new(filename).file_stem().map_or_else(
            || "imported".into(),
            |s| {
                s.to_string_lossy()
                    .to_ascii_lowercase()
                    .replace([' ', '_'], "-")
            },
        )
    } else {
        name.trim().to_owned()
    };
    let colours = match format {
        Format::WarpYaml => warp_yaml(text),
        Format::Ghostty => ghostty(text),
        Format::Alacritty => alacritty(text)?,
        Format::Iterm2 => iterm2(text),
        Format::Base16 => base16(text),
    };
    build(format, name, &colours)
}

/// Normalised keys every reader fills: `background`, `foreground`,
/// `cursor`, `selection` (optional), `accent` (optional), `n0`…`n7`, `b0`…`b7`.
type Colours = BTreeMap<String, String>;

const ANSI_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

fn build(format: Format, name: String, c: &Colours) -> Result<Theme, ImportError> {
    let get = |key: &str| -> Result<Hex, ImportError> {
        let raw = c.get(key).ok_or_else(|| ImportError::Missing {
            format,
            key: key.to_owned(),
        })?;
        normalise(raw)
            .map(Hex)
            .ok_or_else(|| ImportError::BadColour {
                format,
                key: key.to_owned(),
                value: raw.clone(),
            })
    };
    let ansi = |prefix: &str| -> Result<Ansi8, ImportError> {
        Ok(Ansi8 {
            black: get(&format!("{prefix}0"))?,
            red: get(&format!("{prefix}1"))?,
            green: get(&format!("{prefix}2"))?,
            yellow: get(&format!("{prefix}3"))?,
            blue: get(&format!("{prefix}4"))?,
            magenta: get(&format!("{prefix}5"))?,
            cyan: get(&format!("{prefix}6"))?,
            white: get(&format!("{prefix}7"))?,
        })
    };
    let background = get("background")?;
    let foreground = get("foreground")?;
    let normal = ansi("n")?;
    let bright = ansi("b")?;
    let cursor = get("cursor")
        .or_else(|_| get("accent"))
        .unwrap_or_else(|_| foreground.clone());
    let selection = get("selection").unwrap_or_else(|_| Hex(mix(&background, &foreground, 0.2)));
    let accent = get("accent").unwrap_or_else(|_| normal.blue.clone());
    Ok(Theme {
        name,
        background,
        foreground,
        cursor,
        selection,
        ui: Ui {
            accent,
            warning: normal.yellow.clone(),
            danger: normal.red.clone(),
            success: normal.green.clone(),
        },
        normal,
        bright,
    })
}

/// `#rrggbb` from `#rgb`, `rrggbb`, `0xrrggbb` or `#rrggbbaa`.
fn normalise(raw: &str) -> Option<String> {
    let s = raw.trim().trim_matches(|c| c == '\'' || c == '"');
    let s = s
        .strip_prefix('#')
        .or_else(|| s.strip_prefix("0x"))
        .unwrap_or(s);
    let digits: String = s.chars().take_while(char::is_ascii_hexdigit).collect();
    match digits.len() {
        6 | 8 => Some(format!("#{}", digits[..6].to_ascii_lowercase())),
        3 => Some(format!(
            "#{}",
            digits
                .chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_ascii_lowercase()
        )),
        _ => None,
    }
}

/// `a` blended towards `b` by `t`, as `#rrggbb`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=255 first
fn mix(a: &Hex, b: &Hex, t: f64) -> String {
    let (ar, ag, ab) = a.rgb().unwrap_or((0, 0, 0));
    let (br, bg, bb) = b.rgb().unwrap_or((255, 255, 255));
    let ch = |x: u8, y: u8| -> u8 {
        let v = f64::from(x) * (1.0 - t) + f64::from(y) * t;
        // Clamped by construction: both inputs are bytes and t is in 0..=1.
        v.round().clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(ar, br), ch(ag, bg), ch(ab, bb))
}

/// The line without its comment: `#` counts only at the start or after
/// whitespace, so `'#1b1d23'` survives.
fn strip_comment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    match line.find(" #").or_else(|| line.find("\t#")) {
        Some(i) => &line[..i],
        None => line,
    }
}

/// A two-level YAML reader for the shapes Warp and base16 use:
/// `key: value` lines, nesting by indentation, quotes optional.
fn yaml_pairs(text: &str) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for raw in text.lines() {
        let line = strip_comment(raw).trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let Some((key, value)) = line.trim_start().split_once(':') else {
            continue;
        };
        let key = key
            .trim()
            .trim_matches(|c| c == '\'' || c == '"')
            .to_owned();
        let value = value.trim();
        while stack.last().is_some_and(|(i, _)| *i >= indent) {
            stack.pop();
        }
        if value.is_empty() {
            stack.push((indent, key));
        } else {
            let mut path: Vec<String> = stack.iter().map(|(_, k)| k.clone()).collect();
            path.push(key);
            out.push((
                path,
                value.trim_matches(|c| c == '\'' || c == '"').to_owned(),
            ));
        }
    }
    out
}

fn warp_yaml(text: &str) -> Colours {
    let mut c = Colours::new();
    for (path, value) in yaml_pairs(text) {
        let joined = path.join(".");
        match joined.as_str() {
            "background" | "foreground" | "accent" => {
                c.insert(joined, value);
            }
            _ => {
                if let Some(rest) = joined.strip_prefix("terminal_colors.") {
                    let (group, colour) = rest.split_once('.').unwrap_or(("", rest));
                    if let Some(i) = ANSI_NAMES.iter().position(|n| *n == colour) {
                        let prefix = if group == "bright" { "b" } else { "n" };
                        c.insert(format!("{prefix}{i}"), value);
                    }
                }
            }
        }
    }
    c
}

fn base16(text: &str) -> Colours {
    let mut base = BTreeMap::new();
    for (path, value) in yaml_pairs(text) {
        if let Some(key) = path.last()
            && key.len() == 6
            && key.starts_with("base")
        {
            base.insert(key.to_ascii_lowercase(), value);
        }
    }
    let g = |k: &str| base.get(k).cloned().unwrap_or_default();
    let mut c = Colours::new();
    c.insert("background".into(), g("base00"));
    c.insert("foreground".into(), g("base05"));
    c.insert("cursor".into(), g("base05"));
    c.insert("selection".into(), g("base02"));
    c.insert("accent".into(), g("base0d"));
    for (i, k) in [
        "base00", "base08", "base0b", "base0a", "base0d", "base0e", "base0c", "base05",
    ]
    .iter()
    .enumerate()
    {
        c.insert(format!("n{i}"), g(k));
    }
    for (i, k) in [
        "base03", "base08", "base0b", "base0a", "base0d", "base0e", "base0c", "base07",
    ]
    .iter()
    .enumerate()
    {
        c.insert(format!("b{i}"), g(k));
    }
    c
}

fn ghostty(text: &str) -> Colours {
    let mut c = Colours::new();
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "background" | "foreground" => {
                c.insert(key.to_owned(), value.to_owned());
            }
            "cursor-color" => {
                c.insert("cursor".into(), value.to_owned());
            }
            "selection-background" => {
                c.insert("selection".into(), value.to_owned());
            }
            "palette" => {
                if let Some((index, colour)) = value.split_once('=')
                    && let Ok(i) = index.trim().parse::<usize>()
                    && i < 16
                {
                    let key = if i < 8 {
                        format!("n{i}")
                    } else {
                        format!("b{}", i - 8)
                    };
                    c.insert(key, colour.trim().to_owned());
                }
            }
            _ => {}
        }
    }
    c
}

fn alacritty(text: &str) -> Result<Colours, ImportError> {
    let doc: toml::Value =
        toml::from_str(text).map_err(|_| ImportError::UnknownFormat("alacritty toml".into()))?;
    let colors = doc.get("colors").unwrap_or(&doc);
    let mut c = Colours::new();
    let pick = |table: &str, key: &str| -> Option<String> {
        colors.get(table)?.get(key)?.as_str().map(str::to_owned)
    };
    if let Some(v) = pick("primary", "background") {
        c.insert("background".into(), v);
    }
    if let Some(v) = pick("primary", "foreground") {
        c.insert("foreground".into(), v);
    }
    if let Some(v) = pick("cursor", "cursor") {
        c.insert("cursor".into(), v);
    }
    if let Some(v) = pick("selection", "background") {
        c.insert("selection".into(), v);
    }
    for (i, name) in ANSI_NAMES.iter().enumerate() {
        if let Some(v) = pick("normal", name) {
            c.insert(format!("n{i}"), v);
        }
        if let Some(v) = pick("bright", name) {
            c.insert(format!("b{i}"), v);
        }
    }
    Ok(c)
}

/// `.itermcolors`: `<key>Ansi 3 Color</key><dict>` with `Red/Green/Blue
/// Component` reals in 0..=1. Read by walking `<key>` tags in order.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=1 first
fn iterm2(text: &str) -> Colours {
    let mut c = Colours::new();
    let mut current: Option<String> = None;
    let mut rgb = [None::<f64>; 3];
    for piece in text.split("<key>").skip(1) {
        let Some((key, rest)) = piece.split_once("</key>") else {
            continue;
        };
        let key = key.trim();
        if key.ends_with(" Color") {
            current = Some(key.to_owned());
            rgb = [None; 3];
            continue;
        }
        let slot = match key {
            "Red Component" => 0,
            "Green Component" => 1,
            "Blue Component" => 2,
            _ => continue,
        };
        let value = rest
            .split_once("<real>")
            .and_then(|(_, r)| r.split_once("</real>"))
            .and_then(|(v, _)| v.trim().parse::<f64>().ok());
        rgb[slot] = value;
        if let (Some(name), Some(r), Some(g), Some(b)) = (&current, rgb[0], rgb[1], rgb[2]) {
            // 0..=1 reals to bytes; the clamp keeps out-of-range files honest.
            let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let hex = format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b));
            let normalised = match name.as_str() {
                "Background Color" => Some("background".to_owned()),
                "Foreground Color" => Some("foreground".to_owned()),
                "Cursor Color" => Some("cursor".to_owned()),
                "Selection Color" => Some("selection".to_owned()),
                other => other
                    .strip_prefix("Ansi ")
                    .and_then(|s| s.strip_suffix(" Color"))
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|i| *i < 16)
                    .map(|i| {
                        if i < 8 {
                            format!("n{i}")
                        } else {
                            format!("b{}", i - 8)
                        }
                    }),
            };
            if let Some(k) = normalised {
                c.insert(k, hex);
            }
            current = None;
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    const WARP: &str = "accent: '#8ab4f8'\nbackground: '#1b1d23'\ndetails: darker\nforeground: '#dcdfe4'\nterminal_colors:\n  bright:\n    black: '#5c6270'\n    blue: '#8ac4ff'\n    cyan: '#86e5ee'\n    green: '#a3e39f'\n    magenta: '#e0a6ff'\n    red: '#ff8a92'\n    white: '#eceff4'\n    yellow: '#f2d28c'\n  normal:\n    black: '#23262e'\n    blue: '#6fb3f2'\n    cyan: '#6cd3de'\n    green: '#8fd18a'\n    magenta: '#d08ff0'\n    red: '#f07178'\n    white: '#b4b9c4'\n    yellow: '#e6c07b'\n";

    #[test]
    fn warp_yaml_maps_accent_to_cursor_and_ui() {
        let t = import(WARP, "My Warp Theme.yaml", "", None).unwrap();
        assert_eq!(t.name, "my-warp-theme");
        assert_eq!(t.background.0, "#1b1d23");
        assert_eq!(t.cursor.0, "#8ab4f8");
        assert_eq!(t.ui.accent.0, "#8ab4f8");
        assert_eq!(t.normal.red.0, "#f07178");
        assert_eq!(t.bright.white.0, "#eceff4");
        assert_eq!(
            t.selection.0,
            mix(&t.background, &t.foreground, 0.2),
            "no selection in the format: a blend"
        );
        assert!(t.warnings().is_empty(), "{:?}", t.warnings());
    }

    #[test]
    fn ghostty_palette_lines() {
        let mut text = String::from(
            "background = 282c34\nforeground = #abb2bf\ncursor-color = #528bff\nselection-background = #3e4451\n",
        );
        for i in 0..16u32 {
            use std::fmt::Write as _;
            let _ = writeln!(
                text,
                "palette = {i}=#{:02x}{:02x}{:02x}",
                i * 10,
                100 + i,
                200 - i
            );
        }
        let t = import(&text, "one-dark", "od", None).unwrap();
        assert_eq!(t.name, "od");
        assert_eq!(t.background.0, "#282c34");
        assert_eq!(t.cursor.0, "#528bff");
        assert_eq!(t.normal.red.0, "#0a65c7");
        assert_eq!(t.bright.white.0, "#9673b9");
    }

    #[test]
    fn alacritty_toml_and_base16_yaml() {
        let toml = "[colors.primary]\nbackground = '#1d1f21'\nforeground = '#c5c8c6'\n[colors.cursor]\ncursor = '#c5c8c6'\n[colors.normal]\nblack = '#1d1f21'\nred = '#cc6666'\ngreen = '#b5bd68'\nyellow = '#f0c674'\nblue = '#81a2be'\nmagenta = '#b294bb'\ncyan = '#8abeb7'\nwhite = '#c5c8c6'\n[colors.bright]\nblack = '#969896'\nred = '#cc6666'\ngreen = '#b5bd68'\nyellow = '#f0c674'\nblue = '#81a2be'\nmagenta = '#b294bb'\ncyan = '#8abeb7'\nwhite = '#ffffff'\n";
        let t = import(toml, "tomorrow-night.toml", "", None).unwrap();
        assert_eq!(t.normal.blue.0, "#81a2be");
        assert_eq!(t.bright.white.0, "#ffffff");
        let b16 = "scheme: \"Default Dark\"\nauthor: x\nbase00: \"181818\"\nbase01: \"282828\"\nbase02: \"383838\"\nbase03: \"585858\"\nbase04: \"b8b8b8\"\nbase05: \"d8d8d8\"\nbase06: \"e8e8e8\"\nbase07: \"f8f8f8\"\nbase08: \"ab4642\"\nbase09: \"dc9656\"\nbase0A: \"f7ca88\"\nbase0B: \"a1b56c\"\nbase0C: \"86c1b9\"\nbase0D: \"7cafc2\"\nbase0E: \"ba8baf\"\nbase0F: \"a16946\"\n";
        let t = import(b16, "default-dark.yaml", "", None).unwrap();
        assert_eq!(t.background.0, "#181818");
        assert_eq!(t.normal.red.0, "#ab4642");
        assert_eq!(t.bright.black.0, "#585858");
        assert_eq!(t.selection.0, "#383838");
    }

    #[test]
    fn iterm2_plist_reals() {
        let colour = |name: &str, r: f64, g: f64, b: f64| {
            format!(
                "<key>{name}</key>\n<dict>\n<key>Blue Component</key>\n<real>{b}</real>\n<key>Green Component</key>\n<real>{g}</real>\n<key>Red Component</key>\n<real>{r}</real>\n</dict>\n"
            )
        };
        let mut text = String::from("<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n");
        for i in 0..16 {
            text += &colour(&format!("Ansi {i} Color"), f64::from(i) / 16.0, 0.5, 0.25);
        }
        text += &colour("Background Color", 0.0, 0.0, 0.0);
        text += &colour("Foreground Color", 1.0, 1.0, 1.0);
        text += &colour("Cursor Color", 1.0, 0.0, 0.0);
        text += "</dict></plist>";
        let t = import(&text, "Solarized.itermcolors", "", None).unwrap();
        assert_eq!(t.name, "solarized");
        assert_eq!(t.background.0, "#000000");
        assert_eq!(t.cursor.0, "#ff0000");
        assert_eq!(t.normal.red.0, "#108040");
        assert_eq!(t.bright.black.0, "#808040");
    }

    #[test]
    fn missing_colours_and_unknown_formats_are_errors() {
        assert!(matches!(
            import("hello", "notes.txt", "", None),
            Err(ImportError::UnknownFormat(_))
        ));
        let err = import(
            "background: '#000000'\nterminal_colors:\n  normal:\n    red: '#ff0000'\n",
            "x.yaml",
            "",
            None,
        )
        .unwrap_err();
        assert!(matches!(err, ImportError::Missing { .. }), "{err}");
        let bad = import(
            "background = zzz\nforeground = #fff\n",
            "g",
            "",
            Some(Format::Ghostty),
        )
        .unwrap_err();
        assert!(
            matches!(
                bad,
                ImportError::BadColour { .. } | ImportError::Missing { .. }
            ),
            "{bad}"
        );
        assert_eq!(normalise("#ABC"), Some("#aabbcc".into()));
        assert_eq!(normalise("0x11223344"), Some("#112233".into()));
    }
}
