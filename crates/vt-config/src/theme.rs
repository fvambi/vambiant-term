//! Theme files (docs/09 "Themes"): `themes/<name>.toml`, plus the two
//! built-ins that need no file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `#RRGGBB`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Hex(pub String);

impl Hex {
    /// Parses `#RRGGBB` (case-insensitive) into components.
    pub fn rgb(&self) -> Result<(u8, u8, u8), String> {
        let digits = self.0.strip_prefix('#').unwrap_or(&self.0);
        if digits.len() != 6 {
            return Err(format!("{:?} is not #RRGGBB", self.0));
        }
        let packed =
            u32::from_str_radix(digits, 16).map_err(|_| format!("{:?} is not #RRGGBB", self.0))?;
        let [_, red, green, blue] = packed.to_be_bytes();
        Ok((red, green, blue))
    }
}

/// Eight ANSI colours.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Ansi8 {
    pub black: Hex,
    pub red: Hex,
    pub green: Hex,
    pub yellow: Hex,
    pub blue: Hex,
    pub magenta: Hex,
    pub cyan: Hex,
    pub white: Hex,
}

/// UI accents for chrome that is not the grid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Ui {
    pub accent: Hex,
    pub warning: Hex,
    pub danger: Hex,
    pub success: Hex,
}

/// One theme.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Theme {
    pub name: String,
    pub background: Hex,
    pub foreground: Hex,
    pub cursor: Hex,
    pub selection: Hex,
    pub normal: Ansi8,
    pub bright: Ansi8,
    pub ui: Ui,
}

fn h(s: &str) -> Hex {
    Hex(s.into())
}

impl Theme {
    /// `vambiant-dark`, exactly as docs/09 prints it.
    #[must_use]
    pub fn vambiant_dark() -> Self {
        Self {
            name: "vambiant-dark".into(),
            background: h("#0d0f12"),
            foreground: h("#d8dee9"),
            cursor: h("#7aa2f7"),
            selection: h("#2a2f3a"),
            normal: Ansi8 {
                black: h("#1a1d23"),
                red: h("#e06c75"),
                green: h("#98c379"),
                yellow: h("#e5c07b"),
                blue: h("#61afef"),
                magenta: h("#c678dd"),
                cyan: h("#56b6c2"),
                white: h("#abb2bf"),
            },
            bright: Ansi8 {
                black: h("#4b5263"),
                red: h("#ff7b86"),
                green: h("#a9d977"),
                yellow: h("#f0d18a"),
                blue: h("#79c0ff"),
                magenta: h("#d7a3ff"),
                cyan: h("#6fd3de"),
                white: h("#e6e9ef"),
            },
            ui: Ui {
                accent: h("#7aa2f7"),
                warning: h("#e5c07b"),
                danger: h("#e06c75"),
                success: h("#98c379"),
            },
        }
    }

    /// `vambiant-light`: the dark palette's hues on a paper background,
    /// chosen for ≥ 4.5:1 contrast of the foreground.
    #[must_use]
    pub fn vambiant_light() -> Self {
        Self {
            name: "vambiant-light".into(),
            background: h("#f7f8fa"),
            foreground: h("#2e3440"),
            cursor: h("#3b6ee0"),
            selection: h("#d9e2f2"),
            normal: Ansi8 {
                black: h("#2e3440"),
                red: h("#c0392b"),
                green: h("#3f8f3a"),
                yellow: h("#b7791f"),
                blue: h("#2f6fd6"),
                magenta: h("#9b4fc7"),
                cyan: h("#1d8a99"),
                white: h("#d8dee9"),
            },
            bright: Ansi8 {
                black: h("#4c566a"),
                red: h("#d94a3d"),
                green: h("#4ea84a"),
                yellow: h("#c98a2c"),
                blue: h("#3b7fe6"),
                magenta: h("#ad64d9"),
                cyan: h("#2a9db0"),
                white: h("#eceff4"),
            },
            ui: Ui {
                accent: h("#3b6ee0"),
                warning: h("#b7791f"),
                danger: h("#c0392b"),
                success: h("#3f8f3a"),
            },
        }
    }

    /// Both built-ins, keyed by name.
    #[must_use]
    pub fn builtins() -> BTreeMap<String, Self> {
        [Self::vambiant_dark(), Self::vambiant_light()]
            .into_iter()
            .map(|t| (t.name.clone(), t))
            .collect()
    }

    /// Every colour, for contrast checks and previews.
    pub fn colours(&self) -> Vec<(&'static str, &Hex)> {
        let n = &self.normal;
        let b = &self.bright;
        vec![
            ("background", &self.background),
            ("foreground", &self.foreground),
            ("cursor", &self.cursor),
            ("selection", &self.selection),
            ("normal.black", &n.black),
            ("normal.red", &n.red),
            ("normal.green", &n.green),
            ("normal.yellow", &n.yellow),
            ("normal.blue", &n.blue),
            ("normal.magenta", &n.magenta),
            ("normal.cyan", &n.cyan),
            ("normal.white", &n.white),
            ("bright.black", &b.black),
            ("bright.red", &b.red),
            ("bright.green", &b.green),
            ("bright.yellow", &b.yellow),
            ("bright.blue", &b.blue),
            ("bright.magenta", &b.magenta),
            ("bright.cyan", &b.cyan),
            ("bright.white", &b.white),
            ("ui.accent", &self.ui.accent),
            ("ui.warning", &self.ui.warning),
            ("ui.danger", &self.ui.danger),
            ("ui.success", &self.ui.success),
        ]
    }

    /// Problems that are warnings, not rejections (docs/09): colours that
    /// do not parse, and a foreground/background contrast under 4.5:1.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (key, hex) in self.colours() {
            if let Err(e) = hex.rgb() {
                out.push(format!("{}: {key}: {e}", self.name));
            }
        }
        if let (Ok(fg), Ok(bg)) = (self.foreground.rgb(), self.background.rgb()) {
            let ratio = contrast(fg, bg);
            if ratio < 4.5 {
                out.push(format!(
                    "{}: foreground/background contrast is {ratio:.2}:1, below 4.5:1",
                    self.name
                ));
            }
        }
        out
    }
}

fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    fn channel(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// WCAG contrast ratio.
#[must_use]
pub fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_and_pass_contrast() {
        for t in Theme::builtins().values() {
            assert!(t.warnings().is_empty(), "{}: {:?}", t.name, t.warnings());
        }
    }

    #[test]
    fn hex_parses_and_rejects() {
        assert_eq!(h("#7aa2f7").rgb().unwrap(), (0x7a, 0xa2, 0xf7));
        assert!(h("7AA2F7").rgb().is_ok());
        assert!(h("#7aa2").rgb().is_err());
        assert!(h("#zzzzzz").rgb().is_err());
    }

    #[test]
    fn theme_file_round_trips() {
        let t = Theme::vambiant_dark();
        let text = toml::to_string(&t).unwrap();
        let back: Theme = toml::from_str(&text).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn low_contrast_is_a_warning() {
        let mut t = Theme::vambiant_dark();
        t.foreground = h("#1a1d23");
        assert!(t.warnings()[0].contains("contrast"));
    }
}
