//! Reading and editing the files. Parse errors name file, line and key.
//! Edits go through `toml_edit` so the user's comments and layout
//! survive a change made from the settings window.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::keymap::{KeymapFile, Resolved};
use crate::schema::Config;
use crate::theme::Theme;

/// A problem with one file, precise enough to act on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{file}{}: {message}", .line.map_or(String::new(), |l| format!(":{l}")))]
pub struct ConfigError {
    #[allow(missing_docs)]
    pub file: String,
    #[allow(missing_docs)]
    pub line: Option<usize>,
    #[allow(missing_docs)]
    pub message: String,
}

impl ConfigError {
    fn io(file: &Path, e: &std::io::Error) -> Self {
        Self {
            file: file.display().to_string(),
            line: None,
            message: e.to_string(),
        }
    }

    fn parse(file: &Path, text: &str, e: &toml::de::Error) -> Self {
        let line = e
            .span()
            .map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1);
        Self {
            file: file.display().to_string(),
            line,
            message: e.message().to_owned(),
        }
    }
}

/// `~/.config/vambiant-term`, or `VAMBIANT_TERM_CONFIG`.
#[must_use]
pub fn config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("VAMBIANT_TERM_CONFIG") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    home.join(".config/vambiant-term")
}

/// Where each file lives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Paths {
    #[allow(missing_docs)]
    pub config: PathBuf,
    #[allow(missing_docs)]
    pub keymap: PathBuf,
    #[allow(missing_docs)]
    pub themes: PathBuf,
}

impl Paths {
    /// Under `dir`.
    #[must_use]
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            config: dir.join("config.toml"),
            keymap: dir.join("keymap.toml"),
            themes: dir.join("themes"),
        }
    }

    /// Under [`config_dir`].
    #[must_use]
    pub fn default_paths() -> Self {
        Self::in_dir(&config_dir())
    }
}

/// Everything the shell needs, loaded together. A file that fails to
/// parse leaves its `error` set and its value at the defaults — the
/// caller shows the error; nothing silently falls back.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Loaded {
    #[allow(missing_docs)]
    pub paths: Paths,
    #[allow(missing_docs)]
    pub config: Config,
    #[allow(missing_docs)]
    pub config_error: Option<ConfigError>,
    /// `validate()` failures; the config is still returned.
    pub config_warnings: Vec<String>,
    #[allow(missing_docs)]
    pub keymap: Resolved,
    #[allow(missing_docs)]
    pub keymap_error: Option<ConfigError>,
    /// Built-ins plus `themes/*.toml`, by name.
    pub themes: BTreeMap<String, Theme>,
    #[allow(missing_docs)]
    pub theme_warnings: Vec<String>,
}

fn read_optional(path: &Path) -> Result<Option<String>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ConfigError::io(path, &e)),
    }
}

/// Parses `config.toml` text.
pub fn parse_config(path: &Path, text: &str) -> Result<Config, ConfigError> {
    toml::from_str(text).map_err(|e| ConfigError::parse(path, text, &e))
}

/// Loads all three files from `paths`.
#[must_use]
pub fn load(paths: &Paths) -> Loaded {
    let (config, config_error) = match read_optional(&paths.config) {
        Ok(Some(text)) => match parse_config(&paths.config, &text) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(e)),
        },
        Ok(None) => (Config::default(), None),
        Err(e) => (Config::default(), Some(e)),
    };
    let config_warnings = config.validate();
    let (keymap_file, keymap_error) = match read_optional(&paths.keymap) {
        Ok(Some(text)) => match toml::from_str::<KeymapFile>(&text) {
            Ok(k) => (k, None),
            Err(e) => (
                KeymapFile::default(),
                Some(ConfigError::parse(&paths.keymap, &text, &e)),
            ),
        },
        Ok(None) => (KeymapFile::default(), None),
        Err(e) => (KeymapFile::default(), Some(e)),
    };
    let keymap =
        crate::keymap::resolve(config.mux.keymap_profile, &config.mux.prefix, &keymap_file);
    let mut themes = Theme::builtins();
    let mut theme_warnings = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&paths.themes) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "toml") {
                continue;
            }
            match std::fs::read_to_string(&path)
                .map_err(|e| ConfigError::io(&path, &e))
                .and_then(|text| {
                    toml::from_str::<Theme>(&text).map_err(|e| ConfigError::parse(&path, &text, &e))
                }) {
                Ok(theme) => {
                    theme_warnings.extend(theme.warnings());
                    themes.insert(theme.name.clone(), theme);
                }
                Err(e) => theme_warnings.push(e.to_string()),
            }
        }
    }
    for name in [&config.theme.name, &config.theme.light] {
        if !themes.contains_key(name) {
            theme_warnings.push(format!(
                "theme {name:?} is not built in and has no themes/{name}.toml"
            ));
        }
    }
    Loaded {
        paths: paths.clone(),
        config,
        config_error,
        config_warnings,
        keymap,
        keymap_error,
        themes,
        theme_warnings,
    }
}

fn json_to_item(value: &serde_json::Value) -> Result<toml_edit::Item, String> {
    use toml_edit::{Item, Value};
    Ok(match value {
        serde_json::Value::Bool(b) => Item::Value(Value::from(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Item::Value(Value::from(i))
            } else if let Some(f) = n.as_f64() {
                Item::Value(Value::from(f))
            } else {
                return Err(format!("{n} is not a representable number"));
            }
        }
        serde_json::Value::String(s) => Item::Value(Value::from(s.as_str())),
        serde_json::Value::Array(items) => {
            let mut arr = toml_edit::Array::new();
            for item in items {
                match json_to_item(item)? {
                    Item::Value(v) => arr.push(v),
                    _ => return Err("nested tables are not editable values".into()),
                }
            }
            Item::Value(Value::Array(arr))
        }
        serde_json::Value::Null => return Err("null is not a TOML value".into()),
        serde_json::Value::Object(_) => return Err("set one key at a time, not a table".into()),
    })
}

/// Sets `key` (dotted) to `value` in the TOML document `text`, keeping
/// everything else byte-for-byte. Returns the new text.
pub fn set_in_document(text: &str, key: &str, value: &serde_json::Value) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("cannot parse: {e}"))?;
    let parts: Vec<&str> = key.split('.').filter(|p| !p.is_empty()).collect();
    let Some((last, tables)) = parts.split_last() else {
        return Err("empty key".into());
    };
    let mut item = json_to_item(value)?;
    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for part in tables {
        if table.get(part).is_none() {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            table.insert(part, toml_edit::Item::Table(t));
        }
        // Dotted paths descend through `[a.b]` tables and `a = { b = … }`
        // inline tables alike, so the file keeps whichever style it used.
        table = table
            .get_mut(part)
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or_else(|| format!("{part} is not a table"))?;
    }
    // A replaced value keeps its surrounding whitespace and trailing
    // comment; only the value itself changes.
    if let (Some(old), Some(new)) = (
        table.get(last).and_then(toml_edit::Item::as_value),
        item.as_value_mut(),
    ) {
        *new.decor_mut() = old.decor().clone();
    }
    table.insert(last, item);
    Ok(doc.to_string())
}

/// Edits `config.toml`: applies the change to the on-disk document,
/// validates the result as a whole, and writes only if it parses and
/// passes `validate()`. Returns the new config.
pub fn set_config_value(
    paths: &Paths,
    key: &str,
    value: &serde_json::Value,
) -> Result<Config, ConfigError> {
    let file = &paths.config;
    let err = |message: String| ConfigError {
        file: file.display().to_string(),
        line: None,
        message,
    };
    let text = read_optional(file)?.unwrap_or_default();
    let updated = set_in_document(&text, key, value).map_err(|m| err(format!("{key}: {m}")))?;
    let config = parse_config(file, &updated)?;
    let problems = config.validate();
    if !problems.is_empty() {
        return Err(err(problems.join("; ")));
    }
    write_atomically(file, &updated)?;
    Ok(config)
}

/// Edits `keymap.toml`: `chord = action`, or removes the binding when
/// `action` is `None`. Returns the resolved map afterwards.
pub fn set_keymap_binding(
    paths: &Paths,
    config: &Config,
    chord: &str,
    action: Option<&str>,
) -> Result<Resolved, ConfigError> {
    let file = &paths.keymap;
    let err = |message: String| ConfigError {
        file: file.display().to_string(),
        line: None,
        message,
    };
    crate::keymap::parse_chord(chord).map_err(err)?;
    if let Some(action) = action
        && !crate::keymap::ACTIONS
            .iter()
            .any(|(id, _, _)| *id == action)
    {
        return Err(err(format!("{chord:?}: unknown action {action:?}")));
    }
    let text = read_optional(file)?.unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| err(format!("cannot parse: {e}")))?;
    let bindings = doc
        .as_table_mut()
        .entry("bindings")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .ok_or_else(|| err("bindings is not a table".into()))?;
    match action {
        Some(a) => {
            bindings.insert(chord, toml_edit::Item::Value(toml_edit::Value::from(a)));
        }
        None => {
            bindings.remove(chord);
        }
    }
    let updated = doc.to_string();
    let parsed: KeymapFile =
        toml::from_str(&updated).map_err(|e| ConfigError::parse(file, &updated, &e))?;
    write_atomically(file, &updated)?;
    Ok(crate::keymap::resolve(
        config.mux.keymap_profile,
        &config.mux.prefix,
        &parsed,
    ))
}

/// Writes a theme file under `themes/`.
pub fn save_theme(paths: &Paths, theme: &Theme) -> Result<PathBuf, ConfigError> {
    let file = paths.themes.join(format!("{}.toml", theme.name));
    let text = toml::to_string_pretty(theme).map_err(|e| ConfigError {
        file: file.display().to_string(),
        line: None,
        message: e.to_string(),
    })?;
    write_atomically(&file, &text)?;
    Ok(file)
}

fn write_atomically(file: &Path, text: &str) -> Result<(), ConfigError> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| ConfigError::io(dir, &e))?;
    }
    let tmp = file.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(|e| ConfigError::io(&tmp, &e))?;
    std::fs::rename(&tmp, file).map_err(|e| ConfigError::io(file, &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vt-config-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_files_load_defaults_without_errors() {
        let paths = Paths::in_dir(&temp());
        let l = load(&paths);
        assert!(l.config_error.is_none() && l.keymap_error.is_none());
        assert_eq!(l.config, Config::default());
        assert_eq!(l.themes.len(), 2);
        assert!(l.theme_warnings.is_empty(), "{:?}", l.theme_warnings);
    }

    #[test]
    fn parse_errors_name_file_and_line() {
        let dir = temp();
        let paths = Paths::in_dir(&dir);
        std::fs::write(
            &paths.config,
            "[font]\nsize = 13\n[cursor]\nstyle = \"blocky\"\n",
        )
        .unwrap();
        let l = load(&paths);
        let e = l.config_error.expect("error");
        assert!(e.file.ends_with("config.toml"));
        assert_eq!(e.line, Some(4), "{e}");
        assert!(
            e.message.contains("blocky") || e.message.contains("variant"),
            "{e}"
        );
        assert_eq!(
            l.config,
            Config::default(),
            "the value is the default, never a partial file"
        );
    }

    #[test]
    fn set_keeps_comments_and_validates() {
        let dir = temp();
        let paths = Paths::in_dir(&dir);
        std::fs::write(
            &paths.config,
            "# my config\n[font]\nfamily = \"Menlo\" # keep me\nsize = 13.0\n\n[window]\npadding = { x = 8, y = 6 }\n",
        )
        .unwrap();
        let c = set_config_value(&paths, "font.size", &serde_json::json!(14.5)).unwrap();
        assert!((c.font.size - 14.5).abs() < f64::EPSILON);
        let text = std::fs::read_to_string(&paths.config).unwrap();
        assert!(text.starts_with("# my config\n"), "{text}");
        assert!(text.contains("family = \"Menlo\" # keep me"), "{text}");
        assert!(text.contains("size = 14.5\n"), "{text}");
        set_config_value(&paths, "font.family", &serde_json::json!("SF Mono")).unwrap();
        let text = std::fs::read_to_string(&paths.config).unwrap();
        assert!(
            text.contains("family = \"SF Mono\" # keep me"),
            "the edited line keeps its comment: {text}"
        );
        set_config_value(&paths, "window.padding.x", &serde_json::json!(12)).unwrap();
        let l = load(&paths);
        assert_eq!(l.config.window.padding.x, 12);
        assert_eq!(l.config.window.padding.y, 6);
        set_config_value(
            &paths,
            "agents.codex.extra_args",
            &serde_json::json!(["--sandbox", "workspace-write"]),
        )
        .unwrap();
        assert_eq!(
            load(&paths).config.agents.codex.extra_args,
            vec!["--sandbox", "workspace-write"]
        );
    }

    #[test]
    fn set_refuses_invalid_values_and_leaves_the_file_alone() {
        let dir = temp();
        let paths = Paths::in_dir(&dir);
        std::fs::write(&paths.config, "[api]\nport = 7433\n").unwrap();
        let e = set_config_value(&paths, "api.bind", &serde_json::json!("0.0.0.0")).unwrap_err();
        assert!(e.message.contains("api.bind"), "{e}");
        let e = set_config_value(&paths, "cursor.style", &serde_json::json!("blocky")).unwrap_err();
        assert!(
            e.message.contains("blocky") || e.message.contains("variant"),
            "{e}"
        );
        let e = set_config_value(&paths, "font.nope", &serde_json::json!(1)).unwrap_err();
        assert!(e.message.contains("nope"), "{e}");
        assert_eq!(
            std::fs::read_to_string(&paths.config).unwrap(),
            "[api]\nport = 7433\n"
        );
    }

    #[test]
    fn keymap_bindings_are_added_removed_and_checked() {
        let dir = temp();
        let paths = Paths::in_dir(&dir);
        let c = Config::default();
        let r = set_keymap_binding(&paths, &c, "cmd+j", Some("inbox.open")).unwrap();
        assert!(
            r.bindings
                .iter()
                .any(|b| b.chord == "cmd+j" && b.source == "keymap.toml")
        );
        let r = set_keymap_binding(&paths, &c, "cmd+j", None).unwrap();
        assert!(!r.bindings.iter().any(|b| b.chord == "cmd+j"));
        assert!(set_keymap_binding(&paths, &c, "cmd+j", Some("no.such")).is_err());
        assert!(set_keymap_binding(&paths, &c, "hyper+j", Some("inbox.open")).is_err());
    }

    #[test]
    fn theme_files_are_picked_up_and_bad_ones_reported() {
        let dir = temp();
        let paths = Paths::in_dir(&dir);
        let mut t = Theme::vambiant_dark();
        t.name = "mine".into();
        save_theme(&paths, &t).unwrap();
        std::fs::write(paths.themes.join("broken.toml"), "name = 1\n").unwrap();
        let l = load(&paths);
        assert!(l.themes.contains_key("mine"));
        assert!(
            l.theme_warnings.iter().any(|w| w.contains("broken.toml")),
            "{:?}",
            l.theme_warnings
        );
    }
}
