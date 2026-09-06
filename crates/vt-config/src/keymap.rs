//! `keymap.toml` and the two shipped profiles (docs/06 §7). The shell
//! asks the daemon for the *resolved* map — defaults for the active
//! profile(s) with the user's overrides on top — so one table serves the
//! GUI, `vterm keys` and conflict reporting.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::KeymapProfile;

/// `keymap.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeymapFile {
    /// Overrides `[mux] keymap_profile` when present.
    pub profile: Option<KeymapProfile>,
    /// `"cmd+shift+a" = "inbox.open"`, `"prefix a" = "inbox.open"`.
    pub bindings: BTreeMap<String, String>,
}

/// A parsed chord. `prefix` chords are the key after `C-b`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chord {
    /// Key name, lower-case: a letter, digit, symbol or a name such as
    /// `left`, `enter`, `f5`.
    pub key: String,
    #[allow(missing_docs)]
    pub cmd: bool,
    #[allow(missing_docs)]
    pub shift: bool,
    #[allow(missing_docs)]
    pub alt: bool,
    #[allow(missing_docs)]
    pub ctrl: bool,
    #[allow(missing_docs)]
    pub prefix: bool,
}

const KEY_NAMES: &[&str] = &[
    "enter",
    "escape",
    "tab",
    "backspace",
    "space",
    "delete",
    "left",
    "right",
    "up",
    "down",
    "home",
    "end",
    "pageup",
    "pagedown",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
];

/// Parses `cmd+shift+a`, `alt+e`, `prefix a`, `prefix N`, `ctrl+b`.
pub fn parse_chord(text: &str) -> Result<Chord, String> {
    let text = text.trim();
    let (prefix, rest) = match text.strip_prefix("prefix ") {
        Some(r) => (true, r.trim()),
        None => (false, text),
    };
    if rest.is_empty() {
        return Err(format!("{text:?}: no key"));
    }
    let mut chord = Chord {
        key: String::new(),
        cmd: false,
        shift: false,
        alt: false,
        ctrl: false,
        prefix,
    };
    let parts: Vec<&str> = rest.split('+').collect();
    let (mods, key) = parts.split_at(parts.len() - 1);
    for m in mods {
        match m.trim().to_ascii_lowercase().as_str() {
            "cmd" | "command" | "super" => chord.cmd = true,
            "shift" => chord.shift = true,
            "alt" | "opt" | "option" => chord.alt = true,
            "ctrl" | "control" => chord.ctrl = true,
            other => return Err(format!("{text:?}: unknown modifier {other:?}")),
        }
    }
    let key = key[0].trim();
    let mut chars = key.chars();
    let normalised = match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_uppercase() => {
            chord.shift = true;
            c.to_ascii_lowercase().to_string()
        }
        (Some(_), None) => key.to_owned(),
        _ => {
            let lower = key.to_ascii_lowercase();
            let lower = match lower.as_str() {
                "return" => "enter".into(),
                "esc" => "escape".into(),
                _ => lower,
            };
            if !KEY_NAMES.contains(&lower.as_str()) {
                return Err(format!("{text:?}: unknown key {key:?}"));
            }
            lower
        }
    };
    if normalised.is_empty() {
        return Err(format!("{text:?}: no key"));
    }
    chord.key = normalised;
    Ok(chord)
}

impl Chord {
    /// Canonical spelling, parseable by [`parse_chord`].
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.shift {
            parts.push("shift");
        }
        if self.cmd {
            parts.push("cmd");
        }
        parts.push(&self.key);
        let body = parts.join("+");
        if self.prefix {
            format!("prefix {body}")
        } else {
            body
        }
    }
}

/// Every action the shell knows, with the milestone that builds it.
pub const ACTIONS: &[(&str, &str, &str)] = &[
    ("tab.new", "New tab", "M4"),
    (
        "tab.reopen",
        "Reopen the last closed tab (reattach its session)",
        "M5.5",
    ),
    ("tab.rename", "Rename this tab (the session's name)", "M5.5"),
    (
        "pane.sync_toggle",
        "Toggle input sync for this pane",
        "M5.5",
    ),
    ("window.new", "New window", "M4"),
    ("pane.split_right", "Split right", "M4"),
    ("pane.split_down", "Split down", "M4"),
    ("pane.focus_left", "Focus pane left", "M4"),
    ("pane.focus_right", "Focus pane right", "M4"),
    ("pane.focus_up", "Focus pane up", "M4"),
    ("pane.focus_down", "Focus pane down", "M4"),
    ("pane.zoom", "Zoom pane", "M4"),
    ("pane.close", "Close pane (detaches, never kills)", "M4"),
    ("session.detach", "Detach session", "M4"),
    ("session.list", "Session list", "M5"),
    ("inbox.open", "Approval inbox", "M5"),
    ("inbox.next_pending", "Next pending approval", "M5"),
    ("mailbox.open", "Notifications", "M5"),
    (
        "history.search",
        "Search command history (⌃R in the editor)",
        "M5.5",
    ),
    ("palette.open", "Command palette", "M5"),
    ("ai.ask", "⌘K assistant", "M-AI"),
    ("ai.explain_last_failure", "Explain last failure", "M-AI"),
    ("ai.show_last_payload", "Show last egress payload", "M-AI"),
    ("task.new", "New agent task (worktree)", "M6"),
    ("agent.interrupt", "Interrupt agent", "M4"),
    ("scrollback.search", "Search scrollback", "M5"),
    ("prompt.previous", "Jump to previous prompt", "M5"),
    ("prompt.next", "Jump to next prompt", "M5"),
    ("block.select_previous", "Select previous block", "M5"),
    ("block.select_next", "Select next block", "M5"),
    (
        "block.extend_previous",
        "Extend the block selection upwards",
        "M5",
    ),
    (
        "block.extend_next",
        "Extend the block selection downwards",
        "M5",
    ),
    ("block.top", "Scroll to the top of the selected block", "M5"),
    (
        "block.bottom",
        "Scroll to the bottom of the selected block",
        "M5",
    ),
    (
        "block.bookmark",
        "Toggle the selected block's bookmark",
        "M5",
    ),
    (
        "block.bookmark_previous",
        "Jump to the previous bookmark",
        "M5",
    ),
    ("block.bookmark_next", "Jump to the next bookmark", "M5"),
    (
        "block.copy_both",
        "Copy the selected block's command and output",
        "M5",
    ),
    (
        "block.reinput",
        "Put the selected block's command in the prompt",
        "M5",
    ),
    (
        "block.reinput_sudo",
        "Put the selected block's command in the prompt with sudo",
        "M5",
    ),
    ("block.export", "Copy the selected block as HTML", "M5"),
    ("block.menu", "Open the selected block's menu", "M5"),
    ("scrollback.clear", "Clear the scrollback", "M5"),
    ("find.next", "Next find match", "M5"),
    ("find.previous", "Previous find match", "M5"),
    (
        "block.sticky_toggle",
        "Toggle the sticky command header in this pane",
        "M5",
    ),
    ("sidebar.toggle", "Show or hide the sidebar", "M5"),
    (
        "block.copy_command",
        "Copy the selected block's command",
        "M5",
    ),
    (
        "block.copy_output",
        "Copy the selected block's output",
        "M5",
    ),
    ("block.rerun", "Re-run the selected block's command", "M5"),
    ("scrollback.page_up", "Scroll back one page", "M5"),
    ("scrollback.page_down", "Scroll forward one page", "M5"),
    ("scrollback.top", "Scroll to the oldest line", "M5"),
    ("scrollback.bottom", "Scroll to the live end", "M5"),
    ("prefix.send", "Send the prefix key itself", "M4"),
    ("settings.open", "Open Settings", "M4"),
];

/// docs/06 §7, macOS column.
pub const MACOS: &[(&str, &str)] = &[
    ("cmd+t", "tab.new"),
    ("cmd+shift+t", "tab.reopen"),
    ("cmd+alt+i", "pane.sync_toggle"),
    ("cmd+n", "window.new"),
    ("cmd+d", "pane.split_right"),
    ("cmd+shift+d", "pane.split_down"),
    ("cmd+alt+left", "pane.focus_left"),
    ("cmd+alt+right", "pane.focus_right"),
    ("cmd+alt+up", "pane.focus_up"),
    ("cmd+alt+down", "pane.focus_down"),
    ("cmd+shift+enter", "pane.zoom"),
    ("cmd+w", "pane.close"),
    ("cmd+shift+a", "inbox.open"),
    ("cmd+shift+m", "mailbox.open"),
    ("cmd+shift+p", "palette.open"),
    ("cmd+k", "ai.ask"),
    ("alt+e", "ai.explain_last_failure"),
    ("cmd+alt+e", "ai.show_last_payload"),
    ("cmd+shift+n", "task.new"),
    ("cmd+.", "agent.interrupt"),
    ("cmd+f", "scrollback.search"),
    ("cmd+g", "find.next"),
    ("cmd+shift+g", "find.previous"),
    ("cmd+up", "prompt.previous"),
    ("cmd+down", "prompt.next"),
    ("ctrl+cmd+up", "block.select_previous"),
    ("ctrl+cmd+down", "block.select_next"),
    ("ctrl+cmd+shift+up", "block.extend_previous"),
    ("ctrl+cmd+shift+down", "block.extend_next"),
    ("cmd+shift+up", "block.top"),
    ("cmd+shift+down", "block.bottom"),
    ("cmd+b", "block.bookmark"),
    ("alt+up", "block.bookmark_previous"),
    ("alt+down", "block.bookmark_next"),
    ("cmd+shift+c", "block.copy_command"),
    ("cmd+alt+shift+c", "block.copy_output"),
    ("cmd+i", "block.reinput"),
    ("cmd+shift+i", "block.reinput_sudo"),
    ("ctrl+m", "block.menu"),
    ("cmd+shift+k", "scrollback.clear"),
    ("shift+pageup", "scrollback.page_up"),
    ("shift+pagedown", "scrollback.page_down"),
    ("shift+home", "scrollback.top"),
    ("shift+end", "scrollback.bottom"),
    ("cmd+,", "settings.open"),
    ("cmd+\\", "sidebar.toggle"),
];

/// docs/06 §7, tmux column (keys after the prefix).
pub const TMUX: &[(&str, &str)] = &[
    ("prefix c", "tab.new"),
    ("prefix %", "pane.split_right"),
    ("prefix \"", "pane.split_down"),
    ("prefix left", "pane.focus_left"),
    ("prefix right", "pane.focus_right"),
    ("prefix up", "pane.focus_up"),
    ("prefix down", "pane.focus_down"),
    ("prefix z", "pane.zoom"),
    ("prefix d", "session.detach"),
    ("prefix x", "pane.close"),
    ("prefix s", "session.list"),
    ("prefix a", "inbox.open"),
    ("prefix A", "inbox.next_pending"),
    ("prefix m", "mailbox.open"),
    ("prefix :", "palette.open"),
    ("prefix k", "ai.ask"),
    ("prefix e", "ai.explain_last_failure"),
    ("prefix N", "task.new"),
    ("prefix ctrl+c", "agent.interrupt"),
    ("prefix /", "scrollback.search"),
    ("prefix [", "prompt.previous"),
    ("prefix ]", "prompt.next"),
    ("prefix pageup", "scrollback.page_up"),
    ("prefix pagedown", "scrollback.page_down"),
    ("prefix [", "prompt.previous"),
    ("prefix ctrl+b", "prefix.send"),
];

/// One resolved binding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    #[allow(missing_docs)]
    pub chord: String,
    #[allow(missing_docs)]
    pub action: String,
    /// `macos`, `tmux` or `keymap.toml`.
    pub source: String,
}

/// The resolved map plus what is wrong with it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    #[allow(missing_docs)]
    pub profile: KeymapProfile,
    /// The prefix chord, canonical spelling.
    pub prefix: String,
    #[allow(missing_docs)]
    pub bindings: Vec<Binding>,
    /// Overrides that could not be applied, each naming the key.
    pub errors: Vec<String>,
    /// Two actions on one chord, or one action on two chords.
    pub conflicts: Vec<String>,
}

// The enum comes out of a macro shared with the other spellings, so
// `#[default]` cannot be placed on the variant.
#[allow(clippy::derivable_impls)]
impl Default for KeymapProfile {
    fn default() -> Self {
        Self::Tmux
    }
}

/// Applies `file` over the defaults for `profile` (the file's own
/// `profile` wins when set).
#[must_use]
pub fn resolve(profile: KeymapProfile, prefix: &str, file: &KeymapFile) -> Resolved {
    let profile = file.profile.unwrap_or(profile);
    let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut errors = Vec::new();
    let mut add = |chord: &str, action: &str, source: &str, errors: &mut Vec<String>| {
        match parse_chord(chord) {
            Ok(c) => {
                map.insert(c.to_text(), (action.to_owned(), source.to_owned()));
            }
            Err(e) => errors.push(e),
        }
    };
    if matches!(profile, KeymapProfile::Macos | KeymapProfile::Both) {
        for (c, a) in MACOS {
            add(c, a, "macos", &mut errors);
        }
    }
    if matches!(profile, KeymapProfile::Tmux | KeymapProfile::Both) {
        for (c, a) in TMUX {
            add(c, a, "tmux", &mut errors);
        }
    }
    // The prefix itself is always bound, whatever the profile: docs/09
    // `[mux] prefix` says so.
    for (chord, action) in &file.bindings {
        if !ACTIONS.iter().any(|(id, _, _)| id == action) {
            errors.push(format!("keymap.toml: {chord:?}: unknown action {action:?}"));
            continue;
        }
        add(chord, action, "keymap.toml", &mut errors);
    }
    let prefix = parse_chord(prefix).map_or_else(|_| "ctrl+b".to_owned(), |c| c.to_text());
    let mut by_action: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (chord, (action, _)) in &map {
        by_action.entry(action).or_default().push(chord);
    }
    let conflicts = by_action
        .iter()
        .filter(|(_, chords)| chords.len() > 1)
        .map(|(action, chords)| format!("{action} is bound to {}", chords.join(" and ")))
        .collect();
    Resolved {
        profile,
        prefix,
        bindings: map
            .into_iter()
            .map(|(chord, (action, source))| Binding {
                chord,
                action,
                source,
            })
            .collect(),
        errors,
        conflicts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_and_print_canonically() {
        let c = parse_chord("cmd+shift+a").unwrap();
        assert!(c.cmd && c.shift && !c.prefix);
        assert_eq!(c.to_text(), "shift+cmd+a");
        assert_eq!(parse_chord("prefix N").unwrap().to_text(), "prefix shift+n");
        assert_eq!(parse_chord("cmd+.").unwrap().key, ".");
        assert_eq!(parse_chord("cmd+shift+return").unwrap().key, "enter");
        assert!(parse_chord("hyper+x").is_err());
        assert!(parse_chord("cmd+bogus").is_err());
        assert!(parse_chord("").is_err());
    }

    #[test]
    fn default_tables_all_parse_and_name_known_actions() {
        for (chord, action) in MACOS.iter().chain(TMUX) {
            parse_chord(chord).unwrap_or_else(|e| panic!("{e}"));
            assert!(ACTIONS.iter().any(|(id, _, _)| id == action), "{action}");
        }
    }

    #[test]
    fn overrides_win_and_bad_ones_are_named() {
        let mut file = KeymapFile::default();
        file.bindings.insert("cmd+k".into(), "palette.open".into());
        file.bindings.insert("cmd+j".into(), "no.such".into());
        let r = resolve(KeymapProfile::Both, "ctrl+b", &file);
        let k = r.bindings.iter().find(|b| b.chord == "cmd+k").unwrap();
        assert_eq!(
            (k.action.as_str(), k.source.as_str()),
            ("palette.open", "keymap.toml")
        );
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].contains("cmd+j"));
        assert!(
            r.conflicts.iter().any(|c| c.starts_with("palette.open")),
            "{:?}",
            r.conflicts
        );
    }

    #[test]
    fn profile_selects_tables() {
        let r = resolve(KeymapProfile::Macos, "ctrl+b", &KeymapFile::default());
        assert!(r.bindings.iter().all(|b| b.source == "macos"));
        let r = resolve(KeymapProfile::Tmux, "ctrl+a", &KeymapFile::default());
        assert!(r.bindings.iter().all(|b| b.chord.starts_with("prefix ")));
        assert_eq!(r.prefix, "ctrl+a");
    }
}
